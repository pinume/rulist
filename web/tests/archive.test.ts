import assert from "node:assert/strict"
import test from "node:test"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("archive downloads decode correctly and cancel network, input and output failures", async () => {
  const globals = ["document", "window", "localStorage", "fetch"] as const
  const descriptors = globals.map((key) =>
    Object.getOwnPropertyDescriptor(globalThis, key),
  )
  Object.defineProperties(globalThis, {
    document: {
      configurable: true,
      value: { addEventListener() {}, documentElement: { style: {} } },
    },
    window: {
      configurable: true,
      value: {
        location: { origin: "http://localhost" },
        HTMLElement() {},
        WritableStream,
      },
    },
    localStorage: {
      configurable: true,
      value: { getItem: () => null, setItem() {} },
    },
  })
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    ssr: { external: ["streamsaver"] },
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const { r } = await server.ssrLoadModule("/src/utils/request.ts")
    const { default: saver } = await import("streamsaver")
    const { downloadArchive } = await server.ssrLoadModule(
      "/src/utils/archive.ts",
    )
    const entries = ["a.txt", "b.txt", "c.txt", "d.txt"].map((name) => ({
      name,
      is_dir: false,
    }))
    r.defaults.adapter = async (config: any) => ({
      data: { code: 200, data: { url: `/d${JSON.parse(config.data).path}` } },
      status: 200,
      statusText: "OK",
      headers: {},
      config,
    })
    let mode = "success"
    let cancelled = 0
    let started = 0
    let chunks: Uint8Array[] = []
    saver.createWriteStream = () =>
      new WritableStream({
        write(chunk) {
          if (mode === "sink") throw new Error("sink failed")
          chunks.push(chunk)
        },
      })
    Object.defineProperty(globalThis, "fetch", {
      configurable: true,
      value: async (url: string, { signal }: { signal: AbortSignal }) => {
        signal.throwIfAborted()
        if (mode === "network" && url.endsWith("/b.txt"))
          throw new Error("prefetch failed")
        if (["network", "pending"].includes(mode)) {
          started++
          return new Promise((_resolve, reject) =>
            signal.addEventListener(
              "abort",
              () => {
                cancelled++
                reject(signal.reason)
              },
              { once: true },
            ),
          )
        }
        if (mode === "read")
          return new Response(
            new ReadableStream({
              pull(controller) {
                controller.error(new Error("read failed"))
              },
            }),
          )
        return new Response(new Blob([url.split("/").at(-1)!]).stream())
      },
    })
    const progress: any[] = []
    await downloadArchive("/root", entries, {
      signal: new AbortController().signal,
      onProgress: (update: any) => progress.push(update),
    })
    assert.equal(progress.at(-1).current, 4)
    const zip = spawnSync(
      "python3",
      [
        "-c",
        "import sys,io,zipfile; z=zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())); assert z.testzip() is None; assert z.namelist()==['a.txt','b.txt','c.txt','d.txt']; assert z.read('a.txt')==b'a.txt'",
      ],
      { input: Buffer.concat(chunks), encoding: "utf8", timeout: 5000 },
    )
    assert.ifError(zip.error)
    assert.equal(zip.status, 0, zip.stderr)
    for (const failure of ["network", "read", "sink"]) {
      mode = failure
      chunks = []
      await assert.rejects(
        downloadArchive("/root", entries, {
          signal: new AbortController().signal,
          onProgress() {},
        }),
        new RegExp(
          failure === "network" ? "prefetch failed" : `${failure} failed`,
        ),
      )
    }
    assert.ok(cancelled > 0)
    mode = "pending"
    const controller = new AbortController()
    const downloading = downloadArchive("/root", entries, {
      signal: controller.signal,
      onProgress() {},
    })
    const rejection = assert.rejects(downloading, /stopped/)
    const before = started
    for (let i = 0; i < 20 && started === before; i++)
      await new Promise((resolve) => setTimeout(resolve))
    assert.ok(started > before)
    controller.abort(new Error("stopped"))
    await rejection
  } finally {
    await server.close()
    globals.forEach((key, index) => {
      if (descriptors[index])
        Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})
