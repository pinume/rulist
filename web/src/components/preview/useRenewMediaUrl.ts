import { createEffect, createSignal } from "solid-js"
import { PreviewResponse, Resp } from "~/types"
import { r } from "../../utils/request"

export const useRenewMediaUrl = (
  path: () => string,
  initialUrl: () => string,
) => {
  const [rawUrl, setRawUrl] = createSignal(initialUrl())
  let renewedSource = ""

  createEffect(() => setRawUrl(initialUrl()))

  const onError = async (event: Event) => {
    const currentUrl = rawUrl()
    const currentPath = path()
    const source = `${currentPath}:${initialUrl()}`
    if (renewedSource === source) return
    renewedSource = source

    const media = event.currentTarget as HTMLMediaElement
    const currentTime = media.currentTime
    const wasPlaying = !media.paused
    const resp: Resp<PreviewResponse> = await r.post("/fs/preview", {
      path: currentPath,
    })
    const nextUrl = resp.code === 200 ? resp.data?.meta?.raw_url : undefined
    if (
      path() !== currentPath ||
      rawUrl() !== currentUrl ||
      source !== `${path()}:${initialUrl()}`
    )
      return
    if (!nextUrl || nextUrl === currentUrl) return

    const restore = () => {
      if (path() !== currentPath || rawUrl() !== nextUrl) return
      renewedSource = ""
      media.currentTime = currentTime
      if (wasPlaying) void media.play().catch(() => {})
    }
    media.addEventListener("loadedmetadata", restore, { once: true })
    setRawUrl(nextUrl)
    media.load()
  }

  return { rawUrl, onError }
}
