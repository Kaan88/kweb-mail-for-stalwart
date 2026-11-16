package ovh.kweb.mail.ui.filters

/**
 * A small JSON reader and writer for the rule list the core exchanges. Objects keep their key order
 * ([LinkedHashMap]); numbers come back as [Long] or [Double]. No library: org.json is a stub in unit tests.
 */
internal object Json {
    class ParseError(message: String) : Exception(message)

    fun parse(text: String): Any? {
        val p = Parser(text)
        val value = p.value()
        p.skip()
        if (p.i != text.length) throw ParseError("extra text at ${p.i}")
        return value
    }

    @Suppress("UNCHECKED_CAST")
    fun obj(value: Any?): Map<String, Any?>? = value as? Map<String, Any?>

    @Suppress("UNCHECKED_CAST")
    fun arr(value: Any?): List<Any?>? = value as? List<Any?>

    fun write(value: Any?): String = StringBuilder().also { write(value, it) }.toString()

    private fun write(value: Any?, out: StringBuilder) {
        when (value) {
            null -> out.append("null")
            is Boolean -> out.append(value)
            is Long, is Int -> out.append(value.toString())
            is Double -> out.append(if (value % 1.0 == 0.0 && Math.abs(value) < 1e15) value.toLong().toString() else value.toString())
            is String -> string(value, out)
            is Map<*, *> -> {
                out.append('{')
                var first = true
                for ((k, v) in value) {
                    if (!first) out.append(',')
                    first = false
                    string(k as String, out)
                    out.append(':')
                    write(v, out)
                }
                out.append('}')
            }
            is List<*> -> {
                out.append('[')
                value.forEachIndexed { i, v ->
                    if (i > 0) out.append(',')
                    write(v, out)
                }
                out.append(']')
            }
            else -> throw IllegalArgumentException("not JSON: ${value.javaClass}")
        }
    }

    private const val BACKSLASH = '\\'
    private const val QUOTE = '"'

    private fun string(s: String, out: StringBuilder) {
        out.append(QUOTE)
        for (c in s) {
            when {
                c == QUOTE -> out.append(BACKSLASH).append(QUOTE)
                c == BACKSLASH -> out.append(BACKSLASH).append(BACKSLASH)
                c == '\n' -> out.append(BACKSLASH).append('n')
                c == '\r' -> out.append(BACKSLASH).append('r')
                c == '\t' -> out.append(BACKSLASH).append('t')
                c < ' ' -> out.append(BACKSLASH).append("u").append(c.code.toString(16).padStart(4, '0'))
                else -> out.append(c)
            }
        }
        out.append(QUOTE)
    }

    private class Parser(val s: String) {
        var i = 0

        fun skip() {
            while (i < s.length && s[i].isWhitespace()) i++
        }

        fun value(): Any? {
            skip()
            if (i >= s.length) throw ParseError("end of text")
            return when (val c = s[i]) {
                '{' -> objectValue()
                '[' -> arrayValue()
                QUOTE -> stringValue()
                't' -> literal("true", true)
                'f' -> literal("false", false)
                'n' -> literal("null", null)
                else -> if (c == '-' || c.isDigit()) number() else throw ParseError("unexpected $c at $i")
            }
        }

        private fun literal(word: String, v: Any?): Any? {
            if (!s.startsWith(word, i)) throw ParseError("bad literal at $i")
            i += word.length
            return v
        }

        private fun number(): Any {
            val start = i
            while (i < s.length && (s[i].isDigit() || s[i] in "+-.eE")) i++
            val t = s.substring(start, i)
            return t.toLongOrNull() ?: t.toDoubleOrNull() ?: throw ParseError("bad number $t")
        }

        private fun stringValue(): String {
            i++
            val sb = StringBuilder()
            while (true) {
                if (i >= s.length) throw ParseError("unterminated string")
                val c = s[i++]
                when (c) {
                    QUOTE -> return sb.toString()
                    BACKSLASH -> {
                        if (i >= s.length) throw ParseError("unterminated escape")
                        when (val e = s[i++]) {
                            QUOTE, BACKSLASH, '/' -> sb.append(e)
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000c')
                            'u' -> {
                                if (i + 4 > s.length) throw ParseError("bad escape")
                                sb.append(s.substring(i, i + 4).toIntOrNull(16)?.toChar() ?: throw ParseError("bad escape"))
                                i += 4
                            }
                            else -> throw ParseError("bad escape")
                        }
                    }
                    else -> sb.append(c)
                }
            }
        }

        private fun arrayValue(): List<Any?> {
            i++
            val out = ArrayList<Any?>()
            skip()
            if (i < s.length && s[i] == ']') {
                i++
                return out
            }
            while (true) {
                out += value()
                skip()
                if (i >= s.length) throw ParseError("unterminated array")
                if (s[i] == ',') i++ else if (s[i] == ']') { i++; return out } else throw ParseError("bad array at $i")
            }
        }

        private fun objectValue(): Map<String, Any?> {
            i++
            val out = LinkedHashMap<String, Any?>()
            skip()
            if (i < s.length && s[i] == '}') {
                i++
                return out
            }
            while (true) {
                skip()
                if (i >= s.length || s[i] != QUOTE) throw ParseError("key expected at $i")
                val k = stringValue()
                skip()
                if (i >= s.length || s[i] != ':') throw ParseError("colon expected at $i")
                i++
                out[k] = value()
                skip()
                if (i >= s.length) throw ParseError("unterminated object")
                if (s[i] == ',') i++ else if (s[i] == '}') { i++; return out } else throw ParseError("bad object at $i")
            }
        }
    }
}
