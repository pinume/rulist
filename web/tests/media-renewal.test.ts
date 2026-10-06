import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("media renewal treats links as opaque and ignores superseded sources", async () => {
  const globals = ["document", "window", "localStorage"] as const
  const descriptors = globals.map((key) => Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: { addEventListener() {} } },
    window: { configurable: true, value: { location: { origin: "http://localhost" } } },
    localStorage: { configurable: true, value: { getItem: () => null, setItem() {} } },
  })
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { middlewareMode: true }, appType: "custom", logLevel: "silent",
  })
  try {
    const { r } = await server.ssrLoadModule("/src/utils/request.ts")
    const { useRenewMediaUrl } = await server.ssrLoadModule("/src/components/preview/useRenewMediaUrl.ts")
    const pending: ((url: string) => void)[] = []
    r.defaults.adapter = (config: any) => new Promise((resolve) => pending.push((url) => resolve({
      data: { code: 200, data: { meta: { raw_url: url } } },
      status: 200, statusText: "OK", headers: {}, config,
    })))
    const tick = () => new Promise((resolve) => setTimeout(resolve))
    let path = "/a.mp4"
    let loaded: () => void = () => {}
    let loads = 0
    let plays = 0
    const media = { currentTime: 25, paused: false,
      addEventListener(_type: string, callback: () => void) { loaded = callback },
      load() { loads++ }, play() { plays++; return Promise.resolve() },
    }
    const hook = useRenewMediaUrl(() => path, () => "/p/a?opaque-token")
    const event = { currentTarget: media }
    const first = hook.onError(event)
    await tick()
    pending[0]("/p/a?renewed-one")
    await first
    assert.equal(hook.rawUrl(), "/p/a?renewed-one")
    await hook.onError(event)
    assert.equal(pending.length, 1)
    loaded()
    assert.equal(plays, 1)
    const second = hook.onError(event)
    await tick()
    pending[1]("/p/a?renewed-two")
    await second
    assert.equal(hook.rawUrl(), "/p/a?renewed-two")
    assert.equal(loads, 2)
    loaded()
    const stale = hook.onError(event)
    await tick()
    path = "/b.mp4"
    pending[2]("/p/a?stale")
    await stale
    assert.equal(hook.rawUrl(), "/p/a?renewed-two")
    assert.equal(loads, 2)
  } finally {
    await server.close()
    globals.forEach((key, index) => {
      if (descriptors[index]) Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})
