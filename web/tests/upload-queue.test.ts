import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("uploads share concurrency across additions and retries and retain task targets", async () => {
  const globals = ["document", "window", "localStorage"] as const
  const descriptors = globals.map((key) =>
    Object.getOwnPropertyDescriptor(globalThis, key),
  )
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: { addEventListener() {} } },
    window: {
      configurable: true,
      value: { location: { origin: "http://localhost" } },
    },
    localStorage: {
      configurable: true,
      value: { getItem: () => null, setItem() {} },
    },
  })
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const { r } = await server.ssrLoadModule("/src/utils/request.ts")
    const { createUploadQueue } = await server.ssrLoadModule(
      "/src/pages/home/uploads/queue.ts",
    )
    const tick = () => new Promise((resolve) => setTimeout(resolve))
    const requests: {
      path: string
      overwrite: string
      signal: AbortSignal
      respond: (code: number) => void
    }[] = []
    let active = 0
    let maximum = 0
    let idle = 0
    r.defaults.adapter = (config: any) =>
      new Promise((resolve) => {
        maximum = Math.max(maximum, ++active)
        requests.push({
          path: decodeURIComponent(config.headers.get("File-Path")),
          overwrite: config.headers.get("Overwrite"),
          signal: config.signal,
          respond: (code) => {
            active--
            resolve({
              data: { code, message: code === 200 ? "success" : "failed" },
              status: 200,
              statusText: "OK",
              headers: {},
              config,
            })
          },
        })
      })
    const queue = createUploadQueue(() => {
      idle++
    })
    const files = (names: string[]) =>
      names.map((name) => new File([name], name))
    queue.add(files(["a.txt", "b.txt", "c.txt"]), "/first", false)
    queue.add(files(["d.txt", "e.txt", "f.txt"]), "/second", true)
    await tick()
    assert.equal(requests.length, 3)
    requests[0].respond(500)
    await tick()
    assert.equal(queue.state.uploads[0].status, "error")
    assert.equal(requests.length, 4)
    queue.retry(0)
    queue.retry(0)
    await tick()
    assert.equal(requests.length, 4)
    for (let index = 1; index < 7; index++) {
      requests[index].respond(200)
      await tick()
    }
    assert.equal(requests.length, 7)
    assert.equal(maximum, 3)
    assert.equal(idle, 1)
    assert.equal(requests[6].path, "/first/a.txt")
    assert.equal(requests[6].overwrite, "false")
    assert.equal(requests[3].path, "/second/d.txt")
    assert.equal(requests[3].overwrite, "true")
    assert.ok(
      queue.state.uploads.every((task: any) => task.status === "success"),
    )
    queue.clearCompleted()
    assert.deepEqual(queue.state.uploads, [])
    queue.add(
      files(["pending.txt", "waiting.txt", "last.txt", "queued.txt"]),
      "/third",
      false,
    )
    await tick()
    const beforeDispose = requests.length
    queue.dispose()
    for (const request of requests.slice(7)) {
      assert.equal(request.signal.aborted, true)
      request.respond(200)
    }
    await tick()
    assert.equal(requests.length, beforeDispose)
    assert.equal(idle, 1)
  } finally {
    await server.close()
    globals.forEach((key, index) => {
      if (descriptors[index])
        Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})
