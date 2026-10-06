import type { FileState } from "./files"
import { encodePath } from "../utils/path"

type History = { state: FileState; scroll: number }
const history = new Map<string, History>()
const HISTORY_LIMIT = 50

export const readHistory = (path: string) => {
  const key = encodePath(path)
  const entry = history.get(key)
  if (entry) {
    history.delete(key)
    history.set(key, entry)
  }
  return entry
}

export const saveHistory = (path: string, entry: History) => {
  const key = encodePath(path)
  history.delete(key)
  history.set(key, entry)
  if (history.size > HISTORY_LIMIT) history.delete(history.keys().next().value!)
}

export const clearHistory = (path: string) => history.delete(encodePath(path))
export const clearAllHistory = () => history.clear()

// Direct links request fresh data; browser back/forward can restore snapshots.
document.addEventListener(
  "click",
  (event) => {
    const path = (event.target as HTMLElement)
      .closest("a")
      ?.getAttribute("href")
    if (path?.startsWith("/")) clearHistory(decodeURIComponent(path))
  },
  true,
)
