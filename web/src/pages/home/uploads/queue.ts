import { createStore } from "solid-js/store"
import { pathJoin } from "../../../utils/path"
import { File2Upload } from "./util"
import { StreamUpload } from "./stream"
import type { UploadFileProps } from "./types"

export type UploadTask = UploadFileProps & { id: number }

export const createUploadQueue = (onIdle: () => void) => {
  const [state, setState] = createStore<{ uploads: UploadTask[] }>({
    uploads: [],
  })
  const files = new Map<
    number,
    { file: File; path: string; overwrite: boolean }
  >()
  const pending: number[] = []
  const controller = new AbortController()
  let nextId = 0
  let active = 0
  let disposed = false
  const update = (id: number, data: Partial<UploadFileProps>) => {
    if (!disposed) setState("uploads", (task) => task.id === id, data)
  }
  const drain = () => {
    while (!disposed && active < 3 && pending.length) {
      const id = pending.shift()!
      const input = files.get(id)!
      active++
      update(id, { status: "uploading" })
      void StreamUpload(input, (data) => update(id, data), controller.signal)
        .then(() => {
          update(id, { status: "success", progress: 100 })
          files.delete(id)
        })
        .catch((error) =>
          update(id, {
            status: "error",
            msg: error instanceof Error ? error.message : String(error),
          }),
        )
        .finally(() => {
          active--
          drain()
          if (!disposed && active === 0 && pending.length === 0) onIdle()
        })
    }
  }
  return {
    state,
    add: (items: File[], target: string, overwrite: boolean) => {
      if (disposed) return
      const tasks = items.map((file) => {
        const task = { ...File2Upload(file), id: nextId++ }
        files.set(task.id, {
          file,
          path: pathJoin(target, task.path),
          overwrite,
        })
        pending.push(task.id)
        return task
      })
      setState("uploads", (items) => [...items, ...tasks])
      drain()
    },
    retry: (id: number) => {
      if (
        disposed ||
        !files.has(id) ||
        state.uploads.find((task) => task.id === id)?.status !== "error"
      )
        return
      update(id, { status: "pending", msg: "", progress: 0, speed: 0 })
      pending.push(id)
      drain()
    },
    clearCompleted: () => {
      for (const task of state.uploads) {
        if (["success", "error"].includes(task.status)) files.delete(task.id)
      }
      setState("uploads", (tasks) =>
        tasks.filter((task) => !["success", "error"].includes(task.status)),
      )
    },
    dispose: () => {
      disposed = true
      pending.length = 0
      files.clear()
      controller.abort()
    },
  }
}
