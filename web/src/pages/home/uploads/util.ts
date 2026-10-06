import { bus } from "../../../utils/bus"
import { UploadFileProps } from "./types"

export const traverseFileTree = async (entry: FileSystemEntry) => {
  const res: File[] = []

  const internalProcess = async (entry: FileSystemEntry, path: string) => {
    if (entry.isFile) {
      const file = await new Promise<File>((resolve, reject) =>
        (entry as FileSystemFileEntry).file(resolve, reject),
      )
      res.push(
        new File([file], path + file.name, {
          type: file.type,
          lastModified: file.lastModified,
        }),
      )
      return
    }
    if (!entry.isDirectory) return

    const reader = (entry as FileSystemDirectoryEntry).createReader()
    while (true) {
      const entries = await new Promise<FileSystemEntry[]>((resolve, reject) =>
        reader.readEntries(resolve, reject),
      )
      if (entries.length === 0) return
      for (const child of entries) {
        await internalProcess(child, path + entry.name + "/")
      }
    }
  }
  await internalProcess(entry, "")
  return res
}

export const extractFilesFromDataTransfer = async (
  dataTransfer: DataTransfer | null,
): Promise<File[]> => {
  if (!dataTransfer) return []
  const items = Array.from(dataTransfer.items ?? [])
  const files = Array.from(dataTransfer.files ?? [])

  if (items.length === 0) {
    return files
  }

  const res: File[] = []
  for (const item of items) {
    if (item.kind !== "file") continue
    const entry = item.webkitGetAsEntry?.()
    if (entry?.isDirectory) {
      res.push(...(await traverseFileTree(entry)))
    } else {
      const file = item.getAsFile()
      if (file) res.push(file)
    }
  }
  return res
}

export const File2Upload = (file: File): UploadFileProps => {
  return {
    name: file.name,
    path: file.webkitRelativePath || file.name,
    size: file.size,
    progress: 0,
    speed: 0,
    status: "pending",
  }
}

let pendingFiles: File[] = []
let uploadListenerActive = false

export const setUploadListenerActive = (active: boolean) => {
  uploadListenerActive = active
}

export const enqueueFilesForUpload = (files: File[]) => {
  if (files.length === 0) return
  if (uploadListenerActive) {
    bus.emit("upload_files", files)
  } else {
    pendingFiles.push(...files)
  }
  bus.emit("tool", "upload")
}

export const takePendingFiles = (): File[] => {
  const files = pendingFiles
  pendingFiles = []
  return files
}
