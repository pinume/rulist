import {
  PEmptyResp,
  FsGetResp,
  FsListResp,
  FileEntry,
  DirEntry,
  PResp,
  RenameEntry,
} from "~/types"
import { r } from "./request"

export const fsGet = (path = "/", signal?: AbortSignal): Promise<FsGetResp> =>
  r.post("/fs/get", { path }, { signal })

export type ListOptions = {
  page?: number
  per_page?: number
  order_by?: "name" | "size" | "modified"
  reverse?: boolean
  signal?: AbortSignal
}
export const fsList = (
  path = "/",
  options: ListOptions = {},
): Promise<FsListResp> => {
  const { signal, page = 1, per_page = 0, order_by, reverse } = options
  return r.post(
    "/fs/list",
    { path, page, per_page, order_by, reverse },
    { signal },
  )
}

export const fsDirs = (path = "/"): PResp<DirEntry[]> => {
  return r.post("/fs/dirs", { path })
}

export const fsMkdir = (path: string): PEmptyResp => {
  return r.post("/fs/mkdir", { path })
}

export const fsRename = (
  path: string,
  name: string,
  overwrite: boolean,
): PEmptyResp => {
  return r.post("/fs/rename", { path, name, overwrite })
}

export const fsBatchRename = (
  src_dir: string,
  rename_objects: RenameEntry[],
): PEmptyResp => {
  return r.post("/fs/batch_rename", { src_dir, rename_objects })
}

export type ConflictPolicy = "cancel" | "overwrite" | "skip"

export const fsMove = (
  src_dir: string,
  dst_dir: string,
  names: string[],
  conflict_policy: ConflictPolicy = "cancel",
): PEmptyResp => {
  return r.post("/fs/move", {
    src_dir,
    dst_dir,
    names,
    conflict_policy,
  })
}

export const fsCopy = (
  src_dir: string,
  dst_dir: string,
  names: string[],
  conflict_policy: ConflictPolicy = "cancel",
): PEmptyResp => {
  return r.post("/fs/copy", {
    src_dir,
    dst_dir,
    names,
    conflict_policy,
  })
}

export const fsRemove = (dir: string, names: string[]): PEmptyResp => {
  return r.post("/fs/remove", { dir, names })
}

export const fsLink = (
  path: string,
  signal?: AbortSignal,
): PResp<{ url: string }> => {
  return r.post("/fs/link", { path }, { signal })
}

export const authLogout = (): PEmptyResp => {
  return r.get("/auth/logout") as PEmptyResp
}
