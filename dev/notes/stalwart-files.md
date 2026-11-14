FILENODE SCOUT REPORT. Server: Stalwart 1.1.0 at http://127.0.0.1:18080. Everything below was probed with real requests. "Req" is the JMAP method call, "Resp" is the actual answer. The envelope {"using":["urn:ietf:params:jmap:core","urn:ietf:params:jmap:filenode"(,mail)],"methodCalls":[[name,args,"0"]]} is omitted. Passwords were never printed. Scripts are in /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/fn/ (fnlib.py, set FN_CREDS=<file> to pick the user). Do not use scratchpad/j.py: another process overwrote it.

0. CAPABILITY AND SESSION
- Session top-level capabilities["urn:ietf:params:jmap:filenode"] is {}. The real limits are in accounts[id].accountCapabilities["urn:ietf:params:jmap:filenode"]:
  {"maxFileNodeDepth":64,"maxSizeFileNodeName":255,"forbiddenNameChars":"/<>:\"\\|?*","forbiddenNodeNames":[".","..","CON","PRN","AUX","NUL","COM0".."COM9","LPT0".."LPT9"],"fileNodeQuerySortOptions":["name","size","created","modified","mediaType","nodeType","tree"],"supportedNodeTypes":["Directory","File","SymLink"],"mayCreateTopLevelFileNode":true,"caseInsensitiveNames":false,"webTrashUrl":null,"webUrlTemplate":null,"webWriteUrlTemplate":null}
- primaryAccounts["urn:ietf:params:jmap:filenode"]="c" for tester. Account ids are short strings ("c", "f", ...).
- In a shared (non-personal) account the same capability has "mayCreateTopLevelFileNode":false.
- Core limits: maxSizeUpload 50000000, maxConcurrentUpload 4, maxSizeRequest 10000000, maxCallsInRequest 16, maxObjectsInGet 500, maxObjectsInSet 500, collation algorithms i;ascii-numeric, i;ascii-casemap, i;unicode-casemap. Blob capability: maxSizeBlobSet 7499488.
- downloadUrl "http://127.0.0.1:18080/jmap/download/{accountId}/{blobId}/{name}?accept={type}", uploadUrl ".../jmap/upload/{accountId}/".
- Omitting urn:ietf:params:jmap:filenode from "using" gives {"type":"unknownMethod","description":"Method FileNode/set requires capability urn:ietf:params:jmap:filenode which is not present in the \"using\" property."}.

1. DATA MODEL
- There is NO root node. A fresh account has FileNode/get(ids:null) -> {"accountId":"c","state":"n","list":[],"notFound":[]}. Top-level nodes have "parentId":null. Nothing is named "root". Initial state string is "n".
- Discriminator is the property "@type": "Directory" | "File" (SymLink is advertised, see section 11). There is no "type" property: it is rejected on create ({"type":"invalidProperties","description":"Invalid property.","properties":["type"]}). Create without it fails: {"type":"invalidProperties","description":"@type is required on creation.","properties":["@type"]}. Unknown value: "Unsupported @type.". @type is immutable: "@type is immutable after creation.".
- The media type property is "mediaType" (not "type").
- Directory, full get:
  {"id":"b","parentId":null,"@type":"Directory","name":"Docs","created":"2026-11-14T09:53:47Z","modified":"...","accessed":"...","changed":"...","isSubscribed":true,"myRights":{"mayRead":true,"mayAddChildren":true,"mayRename":true,"mayDelete":true,"mayModifyContent":true,"mayShare":true,"maySetMetadata":true},"shareWith":null,"role":null}
- File, full get (NO myRights, shareWith, role, isSubscribed on files):
  {"id":"c","parentId":"b","@type":"File","blobId":"cdofutw9...","size":12,"name":"hello.txt","mediaType":"text/plain","created":"...","modified":"...","accessed":"...","changed":"...","executable":false}
- Folders have no size and no blobId. Files have no myRights, so file permissions must be taken from the parent directory. A readonly viewer's files show no rights either.
- Properties that do not exist and are silently dropped from get when listed in "properties": type, digest, path, description, hasChildren, totalSize, foo. Only "id" and "@type" are always returned. Unknown properties in a create or update are rejected with invalidProperties (description, keywords, linkTarget).
- Timestamps are UTC strings like "2026-11-14T09:53:47Z".
  - "changed" is server-set on every mutation, including rename, move, role and subscribe changes.
  - "created", "modified" and "accessed" are CLIENT-SETTABLE on update: I set created 2020-01-01T00:00:00Z and modified/accessed 2021-02-03T04:05:06Z and they stuck. The server does NOT auto-bump "modified" when the blob is replaced or on rename.
- blobId handling: the blobId returned by get differs from the one uploaded (the server re-materialises the blob under the node). Example: uploaded "edofutw9...nobq", node blobId "cdofutw9...qmai". Blob ids start with the account id. The node's blobId can be downloaded and works with Blob/get.
- Ids are short alphanumerics, reused-looking ("b","c","7","9","ba"), and are only unique per account.
- State strings are short base32-like tokens ("n","sae","sbm","shq"). Intermediate states can be longer ("rameae").
- "executable" boolean on File, default false, settable.
- Directory roles (get "role", settable on Directory only, null by default): root, home, documents, downloads, temp, pictures, music, videos, trash are accepted. templates, shared, public, inbox, bogus give "Unsupported role.". On a File: "role is only defined for Directory nodes.". Roles are unique per account: {"type":"invalidProperties","description":"Another Directory in this account already has this role.","properties":["role"]}. Setting role "trash" on a folder had no visible side effect. I cleared all roles afterwards.
- "isSubscribed" (Directory) is per-user: files2 set false on tester's shared dir and tester still saw true.

2. CREATE, UPLOAD, DOWNLOAD
- Upload: POST /jmap/upload/{accountId}/ with a Content-Type header and a raw body. Resp 200: {"accountId":"c","blobId":"edofutw9qjalagasibjmgmbha7lpsz9ruy0ukjiklql0gaaoqizvkawa3pqnobq","type":"text/plain","size":12}.
- Uploading into someone else's account id (files2 -> /jmap/upload/c/) returned 400 {"type":"about:blank","status":400,"title":"Invalid Parameters","detail":"One or multiple parameters could not be parsed."}. Upload into your own account and reference the blobId from a node in the shared account instead; that works.
- Create directory. Req: FileNode/set {"accountId":"c","create":{"d1":{"@type":"Directory","name":"Docs"}}}. Resp: {"oldState":"n","newState":"sae","created":{"d1":{"created":"2026-11-14T09:53:47Z","modified":"...","accessed":"...","isSubscribed":true,"changed":"...","myRights":{...7 rights all true},"id":"b"}}}. parentId omitted or null means top level.
- Create file. Req: {"create":{"f1":{"@type":"File","name":"hello.txt","parentId":"b","blobId":"<uploaded>","mediaType":"text/plain"}}}. Resp: {"created":{"f1":{"size":12,"executable":false,"created":"...","modified":"...","accessed":"...","changed":"...","id":"c"}}}. The blobId and mediaType you sent are not echoed, so re-get to learn the node's blobId.
- File create errors:
  - blobId missing: {"type":"invalidProperties","description":"blobId is required for File nodes.","properties":["blobId"]}
  - blobId null: "blobId cannot be null for File nodes."
  - unknown blobId: {"type":"invalidProperties","description":"Field could not be set.","properties":["blobId"]}
  - another account's blob: {"type":"forbidden","description":"You do not have access to blobId c....."}
  - name missing: {"type":"invalidProperties","description":"Missing name.","properties":["name"]}
- mediaType: if omitted on create the node gets "application/octet-stream" even if the upload had text/plain. If given, it is stored as given. The upload Content-Type is not inherited. Invalid value: {"type":"invalidProperties","description":"mediaType is not a valid media type.","properties":["mediaType"]}.
- A 0-byte file works (size 0).
- Creation-id references work inside one request: create {"p":{"@type":"Directory","name":"P1","parentId":"h"},"q":{"@type":"File","name":"in.txt","parentId":"#p","blobId":"...","mediaType":"text/plain"}} created both.
- Download: GET /jmap/download/{accountId}/{blobId}/{name}?accept=text/plain -> 200 with the exact body. Headers: Content-Type = the ?accept value, else application/octet-stream (the node's mediaType is NOT used); Content-Disposition: attachment; filename="hello.txt"; Cache-Control: private, immutable, max-age=31536000. A Range header is ignored (200 with the full body). Any {name} segment works.
- The blobId from the shared account works through either account id in the download URL.
- A blob replaced on update (set "blobId" to a new upload): Resp {"updated":{"c":{"changed":"2026-11-14T09:54:37Z","size":22}}}. The server returns the new size. Sending a "size" that does not match is rejected: {"type":"invalidProperties","description":"size does not match the size of the blob.","properties":["size"]}.
- Update responses return only server-changed properties. A no-op returns null ("updated":{"b":null,...}).

3. RENAME, MOVE, COLLISIONS
- Rename: update {"e":{"name":"renamed.bin"}} -> {"updated":{"e":{"changed":"..."}}}. Move: update {"e":{"parentId":"f"}} -> same shape. Both can be in one update. Moving to top level with "parentId":null works.
- Names are CASE-SENSITIVE (caseInsensitiveNames false): "hello.txt" and "Hello.txt" coexist.
- Collision (create or rename or move into a parent that already has the name): {"type":"alreadyExists","existingId":"c"}. A file name vs a directory name collide too. Top-level duplicate: same.
- A move with a colliding name fails; supplying a new name in the same update succeeds.
- Move errors: parent does not exist or is a File: {"type":"invalidProperties","description":"Parent ID does not exist or is not a folder.","properties":["parentId"]}. Moving a folder into its own descendant: "Circular reference in parent ids." (properties ["parentId"]). Depth: 64 levels succeed, the 65th gives {"type":"invalidProperties","description":"Maximum folder depth exceeded.","properties":["parentId"]}.
- Name validation (all invalidProperties, properties ["name"]):
  - any of / < > : " \ | ? * or a control character such as NUL or newline: "Name contains a forbidden character."
  - ".", "..", CON/PRN/AUX/NUL/COM0-9/LPT0-9 case-insensitive, also with an extension (NUL.txt): "Name is reserved and cannot be used."
  - empty or over 255 OCTETS: "Name must be between 1 and 255 octets." (a 255-byte name was accepted, 256 rejected).
  - Accepted: " " (single space), leading and trailing spaces, ".hidden", "a.", ".git", unicode (ünï.txt, 日本語.txt, emoji, Türkçe dosya ğüşiö.txt).
- Nonexistent id on update: {"type":"notFound"}.

4. DESTROY
- Req: {"destroy":["h"]} on a non-empty folder -> {"notDestroyed":{"h":{"type":"nodeHasChildren","description":"Cannot delete non-empty folder."}}}.
- With "onDestroyRemoveChildren":true as a /set argument: {"destroyed":["j","i","h"]}. The list includes the children, children first. This is the working flag name.
- /changes afterwards lists all three (h,i,j) in destroyed.
- Nonexistent id: {"notDestroyed":{"nonexist":{"type":"notFound"}}}. A read-only sharee: {"type":"forbidden","description":"You do not have permission to perform this operation."}.
- Deletion is immediate. There is no trash behaviour (webTrashUrl null).

5. QUERY
- Req: FileNode/query {"accountId":"c","filter":{...},"sort":[{"property":"name","isAscending":true}],"position":0,"anchor":"d","limit":2,"calculateTotal":true}. Resp: {"accountId":"c","queryState":"sbm","canCalculateChanges":true,"position":0,"ids":["b","c","d","e","f","g"],"total":6}. total is only present with calculateTotal. Default sort is by id/creation. Limit 100000 is clamped and the answer reports "limit":5000. limit 0 gives []. Negative position -2 works (position 4). anchor works (anchor "d", limit 2 -> position 2, ids ["d","e"]).
- Supported filter keys (anything else gives {"type":"unsupportedFilter","description":"<key>"}):
  - parentId (a string only; null or an array gives invalidArguments "invalid type: null, expected a borrowed string"; "" and "root" return [])
  - isTopLevel (true -> top-level nodes)
  - name (exact, case-sensitive match)
  - text (free text; matched "hello world" in the content of c, "second" in the content of d, "Docs" a name; a partial "ello" matched nothing; seems token-based over name and content)
  - nodeType ("File" or "Directory"; "SymLink" and unknown values return [])
  - mediaType (exact, no wildcards)
  - minSize, maxSize
  - createdBefore/createdAfter, modifiedBefore/modifiedAfter, accessedBefore/accessedAfter (UTC date strings)
  - role (accepted), blobId (accepted, matched nothing for a bogus id)
  - Operators AND/OR/NOT with "conditions" work.
- UNSUPPORTED filters: type, @type, hasBlob, isSubscribed, executable, hasChildren, path, parentIds, descendantOf, ancestorOf, inTrash, isShared, shareWith, mayRead, owner, changedAfter, size, after, before, hasParent.
- Listing a folder's children = filter {"parentId":"<id>"} (direct children only). Listing top level = filter {"isTopLevel":true}.
- Sorts. Supported: name (case-insensitive: Docs < hello.txt/Hello.txt < renamed.bin < Sub < Top2), size, created, modified, mediaType, nodeType (Directory before File ascending), tree (depth-first order, folders and their contents together). Not supported (unsupportedSort): accessed, parentId, changed, id. Multi-key sort works: [{"property":"nodeType"},{"property":"name"}] gave folders first, then files. A "collation" key is accepted on name sorts. Ties are ordered by id.

6. CHANGES / QUERYCHANGES
- FileNode/changes {"accountId":"c","sinceState":"n"} -> {"oldState":"n","newState":"sbm","hasMoreChanges":false,"created":["b","c","d","e","f","g"],"updated":[],"destroyed":[],"updatedProperties":null}. Works from "n" (initial). A move, rename or content change appears in "updated".
- maxChanges is honoured: {"sinceState":"sai","maxChanges":2} -> created ["d","e"], hasMoreChanges true, newState "rameae" (an intermediate state to continue from).
- Garbage state: {"type":"invalidArguments","description":"invalid JMAP State at line 1 column 181"}. I did not get a cannotCalculateChanges case.
- FileNode/queryChanges {"sinceQueryState":"sbe","filter":{"parentId":"b"},"sort":[{"property":"name"}]} -> {"oldQueryState":"sbe","newQueryState":"sbm","removed":["d"],"added":[{"id":"d","index":1}]} (an updated item shows as removed plus added). calculateTotal adds "total". "canCalculateChanges" is true in query answers.
- The state counter is per account and shared by all changes in it (shared-account /changes gave created ["w","x"] for the sharee).
- A /set response "oldState"/"newState" tracks the same counter.

7. LIMITS AND ERROR TYPES
- FileStorage config: maxSize 25.0 MB, maxFiles <none>, maxFolders <none> (server global). A 27,262,976-byte file was ACCEPTED by FileNode/set, so FileStorage.maxSize is NOT enforced via JMAP (maybe only WebDAV). Not retested for WebDAV.
- Upload size: over 50,000,000 bytes -> HTTP 400 {"type":"urn:ietf:params:jmap:error:limit","status":400,"detail":"The uploaded file is larger than the server is willing to process.","limit":"maxSizeUpload"}.
- Upload rate/volume quota (per account, rolling window of about 5 minutes): HTTP 429 {"type":"about:blank","status":429,"title":"Quota exceeded","detail":"You have exceeded the blob upload quota of 1000 files or 50000000 bytes."} with headers Retry-After (seconds, e.g. 213), RateLimit-Policy: "blob-upload-files";q=1000, "blob-upload-bytes";q=50000000;qu="content-bytes", RateLimit: "blob-upload-files";r=0;t=213, .... The 50 MB per-window budget is shared by all uploads, so a few large uploads block further uploads. This is the likeliest real-world failure for upload loops.
- Per-account quotas (tested with a user with quotas {"maxFiles":3,"maxFolders":2,"maxDiskQuota":200000}). All come as FileNode/set errors, not HTTP errors:
  - {"type":"overQuota","description":"There are too many folders, please delete some before adding a new one."}
  - {"type":"overQuota","description":"There are too many files, please delete some before adding a new one."}
  - {"type":"overQuota","description":"The disk quota has been exceeded."} (also on a blobId replace update)
- Quota/get (urn:ietf:params:jmap:quota) on that account: {"id":"a","resourceType":"octets","used":150963,"warnLimit":null,"softLimit":null,"hardLimit":200000,"scope":"account","name":"filesq@kweb.test","description":"FileNode quota test","types":["Email","SieveScript","FileNode","CalendarEvent","ContactCard"]}. It is a single octets quota shared with mail. For tester (no quota) Quota/get gives an empty list. File and folder counts are not exposed through Quota/get.
- Over-limit ids: FileNode/get with 501 ids -> {"type":"requestTooLarge","description":"The number of ids requested by the client exceeds the maximum number the server is willing to process in a single method call."}.

8. SHARING (tested with a second account "f" = files2@kweb.test)
- Principals: Principal/get returns only your own principal and Principal/query gives {"type":"forbidden","description":"The administrator has disabled directory queries."}. So you cannot look up a sharee by name through JMAP; an account id (principal id equals account id, e.g. "f") must come from elsewhere.
- shareWith is a map accountId -> rights object on a Directory, only returned to someone with mayShare (others see null). Create with {"@type":"Directory","name":"Shared","shareWith":{"f":{"mayRead":true}}}; get shows all 7 rights explicitly: {"f":{"mayRead":true,"mayAddChildren":false,"mayRename":false,"mayDelete":false,"mayModifyContent":false,"mayShare":false,"maySetMetadata":false}}. Patch syntax works: update {"w":{"shareWith/f/mayAddChildren":true,"shareWith/f/mayModifyContent":true}}. Errors: "Account id zzz is invalid." (no properties key) and "Invalid permission \"bogusRight\"." with properties ["shareWith"].
- Rights list: mayRead, mayAddChildren, mayRename, mayDelete, mayModifyContent, mayShare, maySetMetadata.
- The sharee's session gets accounts["c"] = {"name":"tester@kweb.test","isPersonal":false,"isReadOnly":true,...} with accountCapabilities including urn:ietf:params:jmap:filenode (mayCreateTopLevelFileNode false). The sharee sees the shared directory AND its children (children inherit rights and were visible with myRights that match the parent). Files in the shared tree have no myRights.
- Sharee behaviour (mayRead only): create in dir -> {"type":"forbidden","description":"You are not allowed to create file nodes in this folder."}; rename -> {"type":"forbidden","description":"You are not allowed to rename or move this file node."}; destroy -> forbidden "You do not have permission to perform this operation."; setting isSubscribed was allowed (per-user). After mayAddChildren+mayModifyContent were granted, files2 could create files and folders inside; rename was still forbidden. The sharee's created node lives in the owner's account (ids like "y","z" in account "c").
- Cross-account copy: FileNode/copy exists. Req: {"fromAccountId":"c","accountId":"f","create":{"k":{"id":"y","name":"copied.txt"}}} -> {"fromAccountId":"c","accountId":"f","oldState":"sie","newState":"sii","created":{"k":{"size":18,"mediaType":"text/plain","executable":false,"created":"...","modified":"...","accessed":"...","changed":"...","id":"cb"}},"destroyed":null}. fromAccountId must differ from accountId ("accountId must differ from fromAccountId"). The copy lands at top level by default and the original's timestamps are kept. Copying a folder returned a directory node without children (empty source; children copying unverified). onSuccessDestroyOriginal was honoured as a follow-up FileNode/set and was refused with forbidden for a read-only source.
- Using another user's blobId to create a node: {"type":"forbidden","description":"You do not have access to blobId c....."}.

9. WEBDAV
- Yes, the same nodes are visible: PROPFIND /dav/file/ lists /dav/file/tester%40kweb.test/, and PROPFIND with Depth 1 on /dav/file/tester@kweb.test/ lists Docs/ and Shared/ with creationdate, getetag, getlastmodified, lock support. GET /dav/file/tester@kweb.test/Docs/hello.txt returned the body written via JMAP (200, Content-Type text/markdown, matching the node's mediaType and the latest blob). Writes over WebDAV were not tried.

10. NOT WORKING / NOT TESTED
- SymLink: advertised in supportedNodeTypes but I could not create one. {"@type":"SymLink","name":"lnk","parentId":"w"} gives "target is required for SymLink nodes."; with "target" set to a node id ("c"), "Docs/hello.txt", "hello.txt", "/Docs/hello.txt", "http://x/y", "../c", "#c" each gives {"type":"invalidProperties","description":"Field could not be set.","properties":["target"]}. Unknown how target must be shaped.
- Props "description", "keywords", "size" (when mismatching), "type" are not creatable/updatable.
- Not tested: WebDAV write, folder copy with children, event source / push states for FileNode, patching blob content of a node in a shared account by a non-owner, upload-window length exactly (about 5 minutes seen), webUrlTemplate (null).
- Not enforced: FileStorage maxSize 25 MB on JMAP.

11. WHAT I LEFT ON THE SERVER (local test server only)
- Account tester@kweb.test (id c), top-level tree, state "shq":
  - Docs [b]: empty.txt [7] (0 B, text/plain), hello.txt [c] (22 B, text/markdown, executable true, created blob replaced), Hello.txt [d] (7 B, octet-stream, created 2020-01-01, modified/accessed 2021-02-03), Sub [f]: hello.txt [9] (6 B), renamed.bin [e] (7 B)
  - Photos [2]: data.json [3] (7 B), red.png [0] (82 B, valid 16x16 red PNG), two-mb.bin [ba] (2,097,152 B), "Türkçe dosya ğüşiö.txt" [1] (15 B, text/plain)
  - Shared [w]: shared with account f (files2) with mayRead, mayAddChildren, mayModifyContent; contains Inner [x] (dir), from-files2.txt [y] (18 B, written by files2), dir-by-files2 [z] (dir).
  - Roles all cleared; isSubscribed true on all.
- New accounts created through stalwart-cli as the admin account (admin-account.txt): filesq@kweb.test (account id e; quotas maxFiles 3, maxFolders 2, maxDiskQuota 200000) holding D1 [b] with f0.txt, f1.txt, mid.bin (150,000 B), and D2 [c]; credentials in scratchpad/fn/filesq.txt (mode 600). files2@kweb.test (account id f, no quota) holding copied.txt [cb], from-files2.txt [cc], copiedfolder [cd]; credentials in scratchpad/fn/files2.txt (mode 600). The scratch dir is /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/fn/.
- second@kweb.test (id d) exists but is not mine; I did not touch it.
- tester's blob-upload window was exhausted twice by my tests (now recovered); expect 429s for ~5 min after any heavy upload.
- Temporary test nodes (DelTest, depth 64 chain, Top2, 26 MB file, name probes) were deleted; nothing of theirs remains.

OPEN QUESTIONS FOR THE CONTRACT
1. Correct shape of SymLink "target".
2. Whether a sharee's blobId (from the owner's account) downloads for a user with only mayRead (it did, for files2, through both account ids).
3. FileStorage maxSize is configured at 25 MB yet 26 MB passed via JMAP; unclear if that is a bug or WebDAV-only.
4. Whether modified should be set by the client when replacing content (the server does not bump it).