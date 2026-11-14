#!/usr/bin/env python3
"""A test provider for the AI assistant: the Claude Code CLI behind the OpenAI chat protocol.

    python3 dev/claude_bridge.py            # listens on 127.0.0.1:18090

In the app (or `kweb ai set`) choose "OpenAI-compatible" with the address
http://127.0.0.1:18090/v1, any model name ("default" uses the CLI's own model, otherwise the
name is passed to `claude --model`), and no key. From the emulator, map the port first:
`adb reverse tcp:18090 tcp:18090`.

It answers with the Claude login of whoever runs it, so it is a developer's tool for trying the
assistant against a real model without an API key. It is not part of the app.

What makes it safe to feed mail into:
- the CLI runs with every tool switched off (`--tools ""`) and no MCP servers, in an empty
  directory, with no settings from the user or a project, so text in a mail cannot make it
  read files or run anything;
- it listens on this machine only, unless KWEB_BRIDGE_HOST says otherwise;
- nothing is written anywhere: no session is kept, and the request is passed on standard input,
  not on the command line.
"""

import json
import os
import subprocess
import sys
import tempfile
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HOST = os.environ.get("KWEB_BRIDGE_HOST", "127.0.0.1")
PORT = int(os.environ.get("KWEB_BRIDGE_PORT", "18090"))
CLAUDE = os.environ.get("KWEB_BRIDGE_CLAUDE", "claude")
# The models offered by GET /v1/models; "default" lets the CLI choose.
MODELS = ["default", "haiku", "sonnet", "opus"]


def text_of(content):
    """A message's text, whether the client sent a string or a list of parts."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
    return ""


def split(messages):
    """The system prompt, and the conversation as the one prompt the CLI takes."""
    system = "\n\n".join(text_of(m.get("content")) for m in messages if m.get("role") == "system")
    turns = [m for m in messages if m.get("role") in ("user", "assistant")]
    if len(turns) == 1:
        return system, text_of(turns[0].get("content"))
    earlier = "\n\n".join(f"<{m['role']}>\n{text_of(m.get('content'))}\n</{m['role']}>" for m in turns[:-1])
    last = text_of(turns[-1].get("content")) if turns else ""
    return system, f"Earlier in this conversation:\n\n{earlier}\n\nThe user now says:\n{last}"


def command(model, system):
    args = [
        CLAUDE, "-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages",
        "--no-session-persistence", "--tools", "", "--strict-mcp-config", "--setting-sources", "",
    ]
    if system:
        args += ["--system-prompt", system]
    if model and model != "default":
        args += ["--model", model]
    return args


def pieces(model, system, prompt):
    """Runs the CLI and yields ("text", piece) as the answer is written, then ("done", error or None)."""
    with tempfile.TemporaryDirectory(prefix="kweb-bridge-") as empty:
        try:
            run = subprocess.Popen(
                command(model, system), cwd=empty, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, text=True, encoding="utf-8",
            )
        except OSError as e:
            yield "done", f"the Claude CLI could not be started: {e}"
            return
        try:
            run.stdin.write(prompt)
            run.stdin.close()
            streamed = False
            error = None
            for line in run.stdout:
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                kind = event.get("type")
                if kind == "stream_event":
                    delta = event.get("event", {}).get("delta", {})
                    if delta.get("type") == "text_delta" and delta.get("text"):
                        streamed = True
                        yield "text", delta["text"]
                elif kind == "result":
                    if event.get("is_error"):
                        error = str(event.get("result") or "the Claude CLI reported an error")
                    elif not streamed and event.get("result"):
                        # A CLI that did not stream: the whole answer at the end.
                        yield "text", str(event["result"])
            run.wait()
            if error is None and run.returncode not in (0, None) and not streamed:
                error = f"the Claude CLI ended with status {run.returncode}"
            yield "done", error
        finally:
            if run.poll() is None:
                run.kill()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        # One line per request, never the content: it is somebody's mail.
        sys.stderr.write(f"{time.strftime('%H:%M:%S')} {self.command} {self.path} {args[1] if len(args) > 1 else ''}\n")

    def send_json(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            self.send_json(200, {"object": "list", "data": [{"id": m, "object": "model"} for m in MODELS]})
        else:
            self.send_json(404, {"error": {"message": "not found"}})

    def do_POST(self):
        if not self.path.rstrip("/").endswith("/chat/completions"):
            return self.send_json(404, {"error": {"message": "not found"}})
        try:
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
            messages = body["messages"]
        except (ValueError, KeyError, TypeError):
            return self.send_json(400, {"error": {"message": "the request is not a chat completion"}})
        model = str(body.get("model") or "default")
        system, prompt = split(messages)
        if not prompt.strip():
            return self.send_json(400, {"error": {"message": "there is no user message"}})
        if body.get("stream"):
            self.stream(model, system, prompt)
        else:
            self.whole(model, system, prompt)

    def whole(self, model, system, prompt):
        text, error = "", None
        for kind, value in pieces(model, system, prompt):
            if kind == "text":
                text += value
            else:
                error = value
        if error and not text:
            return self.send_json(502, {"error": {"message": error}})
        choice = {"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}
        self.send_json(200, {"object": "chat.completion", "model": model, "choices": [choice]})

    def stream(self, model, system, prompt):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True

        def event(payload):
            self.wfile.write(b"data: " + (payload if isinstance(payload, bytes) else json.dumps(payload).encode()) + b"\n\n")
            self.wfile.flush()

        try:
            for kind, value in pieces(model, system, prompt):
                if kind == "text":
                    event({"object": "chat.completion.chunk", "model": model,
                           "choices": [{"index": 0, "delta": {"content": value}, "finish_reason": None}]})
                elif value:
                    event({"error": {"message": value}})
                else:
                    event({"object": "chat.completion.chunk", "model": model,
                           "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
            event(b"[DONE]")
        except (BrokenPipeError, ConnectionResetError):
            pass  # the client stopped listening (the user tapped Stop); the CLI is ended by `pieces`


def main():
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"Claude CLI bridge on http://{HOST}:{PORT}/v1 (Ctrl+C to stop)", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
