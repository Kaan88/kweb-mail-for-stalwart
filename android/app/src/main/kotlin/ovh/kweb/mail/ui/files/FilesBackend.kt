package ovh.kweb.mail.ui.files

import java.io.File
import uniffi.kweb_core.Core
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileListing
import uniffi.kweb_core.FileSpace

/** What the Files screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface FilesBackend {
    fun spaces(): List<FileSpace>
    fun files(account: String?, folderId: String?): FileListing
    fun search(account: String?, text: String): List<FileEntry>
    fun createFolder(account: String?, parentId: String?, name: String): FileEntry
    fun upload(account: String?, parentId: String?, name: String, mediaType: String, sourcePath: String, replace: Boolean): FileEntry
    fun rename(account: String?, id: String, name: String)
    fun move(account: String?, id: String, parentId: String?)
    fun delete(account: String?, id: String)
    fun download(account: String?, blobId: String, destPath: String)
}

class CoreFilesBackend(private val core: () -> Core) : FilesBackend {
    override fun spaces() = core().fileSpaces()
    override fun files(account: String?, folderId: String?) = core().files(account, folderId)
    override fun search(account: String?, text: String) = core().fileSearch(account, text)
    override fun createFolder(account: String?, parentId: String?, name: String) = core().fileCreateFolder(account, parentId, name)
    override fun upload(account: String?, parentId: String?, name: String, mediaType: String, sourcePath: String, replace: Boolean) =
        core().fileUpload(account, parentId, name, mediaType, sourcePath, replace)
    override fun rename(account: String?, id: String, name: String) = core().fileRename(account, id, name)
    override fun move(account: String?, id: String, parentId: String?) = core().fileMove(account, id, parentId)
    override fun delete(account: String?, id: String) = core().fileDelete(account, id)
    override fun download(account: String?, blobId: String, destPath: String) = core().fileDownload(account, blobId, destPath)
}

/** The name and size of a document the user picked; [size] is null when the provider does not say. */
data class UploadInfo(val name: String, val size: Long?)

/** A picked document copied to the cache: [path] is what the core reads, [cleanup] removes the copy. */
class PickedUpload(val path: String, val mediaType: String, val size: Long, val cleanup: () -> Unit)

/** What the Files screens need from Android. Every call blocks: run it off the main thread. */
interface FilesPlatform {
    /** Where the download of [entry] goes: under the cache folder the `FileProvider` shares. */
    fun cacheFile(account: String?, entry: FileEntry): File

    /** The display name and size of a picked document (a content URI as text). */
    fun describe(item: String): UploadInfo

    /** Copies the picked document to a temporary file named [name]. */
    fun copyForUpload(item: String, name: String): PickedUpload

    /** Removes copies for upload that a cancelled or killed upload left in the cache. */
    fun sweepLeftovers()

    /** Writes [source] to the document the user chose with the save picker (a content URI as text). */
    fun copyTo(source: File, target: String)

    /** Removes the empty document the save picker made, when nothing could be written to it. */
    fun discard(target: String)
}
