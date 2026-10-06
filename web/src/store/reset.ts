import { resetFileState } from "./files"
import { setCurrentUser, setSessionExpired } from "./session"

export const resetSessionState = () => {
  resetFileState()
  setCurrentUser(null)
  setSessionExpired(false)
}
