import { createMemo, createSignal } from "solid-js"
import { createStore, reconcile } from "solid-js/store"
import { fsGet, fsList } from "../utils/api"
import { pathJoin } from "../utils/path"
import {
  clearAllHistory,
  clearHistory,
  readHistory,
  saveHistory,
} from "./history"
import { FileEntry, FileItem, FileType } from "~/types"

export type OrderBy = "name" | "size" | "modified"
export const LIST_PAGE_SIZE = 100
type SortState = { orderBy: OrderBy; reverse: boolean }
const defaultSort: SortState = { orderBy: "name", reverse: false }
let fileRequestGeneration = 0

let requestController: AbortController | undefined
let loadedPath: string | undefined

const saveSortState = (dir: string, state: SortState) => {
  try {
    localStorage.setItem(`dir_sort_${dir}`, JSON.stringify(state))
  } catch (err) {
    console.warn("failed to save sort config:", err)
  }
}

const loadSortState = (dir: string): SortState => {
  try {
    const item = localStorage.getItem(`dir_sort_${dir}`)
    if (!item) return defaultSort
    const state = JSON.parse(item) as SortState
    if (
      ["name", "size", "modified"].includes(state.orderBy) &&
      typeof state.reverse === "boolean"
    ) {
      return state
    }
  } catch (err) {
    console.warn("failed to read sort config:", err)
  }
  return defaultSort
}

export enum ViewState {
  Initial,
  Loading,
  Folder,
  File,
}
const createInitialFileStore = () => ({
  file: {} as FileEntry,
  files: [] as FileItem[],
  total: 0,
  page: 1,
  orderBy: "name" as OrderBy,
  reverse: false,
  state: ViewState.Initial,
  err: "",
})
export type FileState = ReturnType<typeof createInitialFileStore>
const [fileStore, setFileStore] = createStore<FileState>(
  createInitialFileStore(),
)

const setListing = (files: FileEntry[], total: number, page: number) => {
  if (fileStore.page !== page) setDirectoryFilter("")
  selectAll(false)
  setFileStore("files", reconcile(files))
  setFileStore({ total, page })
  setFileStore("file", "is_dir", true)
}

let lastClickedIndex: number | null = null

export const setLastClickedIndex = (index: number | null) => {
  lastClickedIndex = index
}

const directoryPaths: Record<string, boolean> = {}
export const rememberDirectoryPath = (path: string, dir: boolean) => {
  if (dir) directoryPaths[path] = true
  else delete directoryPaths[path]
}
export const isKnownDirectoryPath = (path: string) =>
  directoryPaths[path] === true

const clearSelection = () => {
  for (const index of fileStore.files.keys()) {
    setFileStore("files", index, { selected: false })
  }
}

export const selectRange = (targetIndex: number, additive = false) => {
  const indexes = visibleFileIndexes()
  if (!indexes.includes(targetIndex)) return
  if (lastClickedIndex === null || !indexes.includes(lastClickedIndex)) {
    if (!additive) clearSelection()
    selectIndex(targetIndex, true)
    lastClickedIndex = targetIndex
    return
  }
  const posA = indexes.indexOf(lastClickedIndex)
  const posB = indexes.indexOf(targetIndex)
  const start = Math.min(posA, posB)
  const end = Math.max(posA, posB)
  if (!additive) clearSelection()
  for (let i = start; i <= end; i++) {
    setFileStore("files", indexes[i], { selected: true })
  }
}

export const selectIndex = (index: number, checked: boolean, one?: boolean) => {
  const indexes = visibleFileIndexes()
  if (!indexes.includes(index)) return
  if (one) selectAll(false)
  setFileStore("files", index, { selected: checked })
}

export const selectAll = (checked: boolean) => {
  if (!checked) {
    lastClickedIndex = null
    clearSelection()
    return
  }
  const indexes = visibleFileIndexes()
  for (const index of indexes)
    setFileStore("files", index, { selected: checked })
}

export const selectedFiles = () =>
  fileStore.files.filter((file) => file.selected)
export const oneSelected = () => selectedNum() === 1

const selectedNum = createMemo(() => selectedFiles().length)
export { fileStore }
const [directoryFilter, setDirectoryFilterValue] = createSignal("")
export const setDirectoryFilter = (value: string) => {
  if (directoryFilter() === value) return
  selectAll(false)
  setDirectoryFilterValue(value)
}
export const clearDirectoryFilter = () => setDirectoryFilter("")
export { directoryFilter }
export const visibleFileIndexes = createMemo(() => {
  const query = directoryFilter().trim().toLowerCase()
  return fileStore.files.flatMap((obj, index) =>
    !query || obj.name.toLowerCase().includes(query) ? [index] : [],
  )
})
const getCountStr = (
  objs: FileItem[],
  prefix: "count" | "selected",
  filterType?: FileType,
) => {
  if (filterType)
    objs = objs.filter((obj) => obj.is_dir || obj.type === filterType)
  if (objs.length === 0) return ""
  const folders = objs.filter((o) => o.is_dir).length
  const files = objs.length - folders
  const label = prefix === "count" ? "This page" : "Selected"
  if (folders && files) return `${label}: ${folders} folders, ${files} files`
  if (folders) return `${label}: ${folders} folders`
  return `${label}: ${files} files`
}

export const countMsg = (filterType?: FileType) =>
  getCountStr(fileStore.files, "count", filterType)

export const selectedMsg = (filterType?: FileType) => {
  const selectedList = selectedFiles()
  return selectedList.length > 0
    ? getCountStr(selectedList, "selected", filterType)
    : ""
}

export const resetFileState = () => {
  fileRequestGeneration++
  requestController?.abort()
  loadedPath = undefined
  clearAllHistory()
  setFileStore(reconcile(createInitialFileStore()))
  setDirectoryFilterValue("")
  lastClickedIndex = null
  for (const path of Object.keys(directoryPaths)) delete directoryPaths[path]
  setUploadConfig({ overwrite: false })
  setShouldKeepState(false)
}

export const [uploadConfig, setUploadConfig] = createStore({
  overwrite: false,
})

export const [shouldKeepState, setShouldKeepState] = createSignal(false)

const waitForFrame = () => new Promise<void>((resolve) => setTimeout(resolve))

const beginLoad = (path: string) => {
  if (
    loadedPath &&
    loadedPath !== path &&
    !fileStore.err &&
    [ViewState.Folder, ViewState.File].includes(fileStore.state)
  ) {
    saveHistory(loadedPath, {
      state: JSON.parse(JSON.stringify(fileStore)),
      scroll: window.scrollY,
    })
  }
  if (loadedPath !== path) {
    lastClickedIndex = null
    setDirectoryFilter("")
  }
  requestController?.abort()
  requestController = new AbortController()
  const generation = ++fileRequestGeneration
  setFileStore("err", "")
  return {
    signal: requestController.signal,
    current: () => generation === fileRequestGeneration,
  }
}

type FileRequest = ReturnType<typeof beginLoad>
const acceptError = (response: { code?: number; message: string }) => {
  if (response.code === undefined || response.code >= 0)
    setFileStore("err", response.message)
}

const fetchFolder = async (
  path: string,
  page: number,
  sort: SortState,
  request: FileRequest,
): Promise<void> => {
  const response = await fsList(path, {
    page,
    per_page: LIST_PAGE_SIZE,
    order_by: sort.orderBy,
    reverse: sort.reverse,
    signal: request.signal,
  })
  if (!request.current()) return
  if (response.code !== 200) {
    acceptError(response)
    return
  }
  const lastPage = Math.max(1, Math.ceil(response.data.total / LIST_PAGE_SIZE))
  if (page > lastPage) return fetchFolder(path, lastPage, sort, request)
  rememberDirectoryPath(path, true)
  for (const item of response.data.content ?? []) {
    if (item.is_dir) rememberDirectoryPath(pathJoin(path, item.name), true)
  }
  setListing(response.data.content ?? [], response.data.total, page)
  if (!shouldKeepState()) setFileStore("state", ViewState.Folder)
  loadedPath = path
}

export const loadPath = async (path: string, page = 1) => {
  const history = readHistory(path)
  const request = beginLoad(path)
  const sort = loadSortState(path)
  setFileStore(sort)
  if (history) {
    if (!shouldKeepState()) setFileStore("state", ViewState.Initial)
    await waitForFrame()
    if (!request.current() || readHistory(path) !== history) return
    setFileStore(reconcile(JSON.parse(JSON.stringify(history.state))))
    loadedPath = path
    await waitForFrame()
    if (request.current() && readHistory(path) === history)
      window.scroll({ top: history.scroll })
    return
  }
  if (!shouldKeepState()) setFileStore("state", ViewState.Loading)
  if (isKnownDirectoryPath(path)) return fetchFolder(path, page, sort, request)
  const response = await fsGet(path, request.signal)
  if (!request.current()) return
  if (response.code !== 200) {
    acceptError(response)
    return
  }
  setFileStore("file", reconcile(response.data))
  if (response.data.is_dir) return fetchFolder(path, page, sort, request)
  if (!shouldKeepState()) setFileStore("state", ViewState.File)
  loadedPath = path
}

export const loadFolder = async (
  path: string,
  page = 1,
  orderBy = fileStore.orderBy,
  reverse = fileStore.reverse,
) => {
  const request = beginLoad(path)
  clearHistory(path)
  setFileStore({ orderBy, reverse })
  if (!shouldKeepState()) setFileStore("state", ViewState.Loading)
  return fetchFolder(path, Math.max(1, page), { orderBy, reverse }, request)
}

export const sortFolder = (
  path: string,
  orderBy: OrderBy,
  reverse: boolean,
) => {
  saveSortState(path, { orderBy, reverse })
  return loadFolder(path, 1, orderBy, reverse)
}

export const refreshFiles = async (
  path: string,
  invalidatePaths: string[] = [],
) => {
  const scroll = window.scrollY
  clearHistory(path)
  for (const invalidatedPath of invalidatePaths) clearHistory(invalidatedPath)
  const pending = loadPath(path, fileStore.page)
  const generation = fileRequestGeneration
  await pending
  if (generation === fileRequestGeneration)
    window.scroll({ top: scroll, behavior: "smooth" })
}
