import {
  loadPath,
  loadFolder,
  sortFolder,
  refreshFiles,
  rememberDirectoryPath,
} from "../store/files"
import { pathJoin } from "../utils/path"
import { useRouter } from "./useRouter"

export const useFiles = () => {
  const { pathname } = useRouter()
  return {
    loadPath,
    loadFolder,
    sort: (orderBy: Parameters<typeof sortFolder>[1], reverse: boolean) =>
      sortFolder(pathname(), orderBy, reverse),
    refresh: (invalidatePaths?: string[]) =>
      refreshFiles(pathname(), invalidatePaths),
    rememberDirectory: (path: string, dir = true, push = false) =>
      rememberDirectoryPath(push ? pathJoin(pathname(), path) : path, dir),
  }
}
