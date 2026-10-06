import streamSaver from "streamsaver"
import createZip from "./zip-stream.js"
import { collectDownloadFiles, getFreshDownloadUrl } from "./download"
import { pathBase, pathJoin } from "./path"
import type { FileEntry } from "../types/file"

streamSaver.mitm = "/streamer/mitm.html"
type Progress = {
  phase: "listing" | "downloading"
  current: number
  total: number
  name?: string
}

export const downloadArchive = async (
  root: string,
  selected: FileEntry[],
  options: { signal: AbortSignal; onProgress: (progress: Progress) => void },
) => {
  const controller = new AbortController()
  const { signal } = controller
  const cancel = () => controller.abort(options.signal.reason)
  options.signal.addEventListener("abort", cancel, { once: true })
  const responses = new Map<number, Promise<Response>>()
  try {
    options.signal.throwIfAborted()
    options.onProgress({ phase: "listing", current: 0, total: 0 })
    const files = await collectDownloadFiles(root, selected, signal)
    signal.throwIfAborted()
    if (!files.length) throw new Error("No files to download")
    if (files.length > 65534)
      throw new Error("Archive exceeds the ZIP entry limit")
    const saveName =
      (selected.length === 1 ? selected[0].name : pathBase(root)) || "Home"
    const concurrency = 4
    const prefetch = (index: number) => {
      let response = responses.get(index)
      if (!response) {
        response = (async () => {
          const url = await getFreshDownloadUrl(
            pathJoin(root, files[index].path),
            signal,
          )
          signal.throwIfAborted()
          const result = await fetch(url, { signal })
          if (!result.ok)
            throw new Error(
              `Failed to fetch ${files[index].path}: ${result.status} ${result.statusText}`,
            )
          return result
        })()
        void response.catch((error) => controller.abort(error))
        responses.set(index, response)
      }
      return response
    }
    const output = streamSaver.createWriteStream(`${saveName}.zip`)
    for (let i = 0; i < Math.min(concurrency, files.length); i++) prefetch(i)
    let index = 0
    const zip = createZip({
      async pull(writer) {
        signal.throwIfAborted()
        if (index === files.length) {
          writer.close()
          return
        }
        const current = index++
        const response = await prefetch(current)
        signal.throwIfAborted()
        if (!response.body)
          throw new Error(`Empty response body for ${files[current].path}`)
        responses.delete(current)
        for (
          let i = current + 1;
          i < Math.min(current + concurrency, files.length);
          i++
        )
          prefetch(i)
        const path = files[current].path.replace(/^\/+|\/+$/g, "")
        const prefix = `${saveName}/`
        const name =
          selected.length === 1 && path.startsWith(prefix)
            ? path.slice(prefix.length)
            : path
        options.onProgress({
          phase: "downloading",
          current: current + 1,
          total: files.length,
          name,
        })
        writer.enqueue({ name, stream: response.body })
      },
      cancel: (reason: unknown) => controller.abort(reason),
    })
    await zip.pipeTo(output, { signal })
  } catch (error) {
    controller.abort(error)
    throw error
  } finally {
    options.signal.removeEventListener("abort", cancel)
    for (const response of responses.values()) {
      void response
        .then((result) => {
          if (result.body && !result.body.locked) return result.body.cancel()
        })
        .catch(() => {})
    }
  }
}
