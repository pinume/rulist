import { EmptyResp } from "~/types"
import { r } from "../../../utils/request"
import type { UploadFileProps } from "./types"

export const StreamUpload = async (
  task: { path: string; file: File; overwrite: boolean },
  onProgress: (update: Partial<UploadFileProps>) => void,
  signal: AbortSignal,
): Promise<void> => {
  let oldTimestamp = Date.now()
  let oldLoaded = 0
  const resp: EmptyResp = await r.put("/fs/put", task.file, {
    signal,
    headers: {
      "File-Path": encodeURIComponent(task.path),
      "Content-Type": task.file.type || "application/octet-stream",
      Overwrite: task.overwrite.toString(),
    },
    onUploadProgress: ({ loaded, total }) => {
      if (!total) return
      const progress = Math.floor((loaded / total) * 100)
      const update: Partial<UploadFileProps> = { progress }
      const timestamp = Date.now()
      const duration = (timestamp - oldTimestamp) / 1000
      if (duration > 1) {
        update.speed = (loaded - oldLoaded) / duration
        oldTimestamp = timestamp
        oldLoaded = loaded
      }
      if (progress === 100) update.status = "backending"
      onProgress(update)
    },
  })
  if (resp.code !== 200) throw new Error(resp.message)
}
