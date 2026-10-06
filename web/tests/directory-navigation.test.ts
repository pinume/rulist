import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("directory navigation tracks known directories and path joins correctly", async () => {
  const globals = ["document", "window", "localStorage"] as const
  const descriptors = globals.map((key) =>
    Object.getOwnPropertyDescriptor(globalThis, key),
  )
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: { addEventListener() {} } },
    window: {
      configurable: true,
      value: {
        location: { origin: "http://localhost" },
        scrollY: 0,
        scroll() {},
      },
    },
    localStorage: {
      configurable: true,
      value: { getItem: () => null, setItem() {} },
    },
  })
  let server: Awaited<ReturnType<typeof createServer>> | undefined

  try {
    const root = fileURLToPath(new URL("..", import.meta.url))
    server = await createServer({
      root,
      configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
      server: { middlewareMode: true },
      appType: "custom",
      logLevel: "silent",
    })
    const files = await server.ssrLoadModule("/src/store/files.ts")
    const pathUtils = await server.ssrLoadModule("/src/utils/path.ts")

    // Verify initial state
    assert.equal(files.isKnownDirectoryPath("/snap"), false)
    assert.equal(files.isKnownDirectoryPath("/rulist"), false)

    // Simulate folder listing marking discovered directories
    files.rememberDirectoryPath("/snap", true)
    files.rememberDirectoryPath("/rulist", true)
    files.rememberDirectoryPath("/rulist/github", true)

    assert.equal(files.isKnownDirectoryPath("/snap"), true)
    assert.equal(files.isKnownDirectoryPath("/rulist"), true)
    assert.equal(files.isKnownDirectoryPath("/rulist/github"), true)
    assert.equal(files.isKnownDirectoryPath("/unknown"), false)

    // Verify path utilities prevent duplicate slashes
    assert.equal(pathUtils.pathJoin("/", "snap"), "/snap")
    assert.equal(pathUtils.pathJoin("/snap", "lxd"), "/snap/lxd")
    assert.equal(pathUtils.pathJoin("/rulist", "github"), "/rulist/github")

    // Verify reset clears known directory paths
    files.resetFileState()
    assert.equal(files.isKnownDirectoryPath("/snap"), false)
    assert.equal(files.isKnownDirectoryPath("/rulist"), false)
  } finally {
    await server?.close()
    globals.forEach((key, index) => {
      if (descriptors[index])
        Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})

test("refresh invalidates the destination snapshot after a file operation", async () => {
  const globals = ["document", "window", "localStorage"] as const
  const descriptors = globals.map((key) =>
    Object.getOwnPropertyDescriptor(globalThis, key),
  )
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: { addEventListener() {} } },
    window: {
      configurable: true,
      value: { location: { origin: "http://localhost" }, scrollY: 0, scroll() {} },
    },
    localStorage: {
      configurable: true,
      value: { getItem: () => null, setItem() {} },
    },
  })
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const files = await server.ssrLoadModule("/src/store/files.ts")
    const { r } = await server.ssrLoadModule("/src/utils/request.ts")
    const contents: Record<string, string[]> = {
      "/dst": ["before.txt"],
      "/src": ["source.txt"],
    }
    let dstRequests = 0
    r.defaults.adapter = async (config: any) => {
      const path = JSON.parse(config.data).path
      if (config.url === "/fs/list" && path === "/dst") dstRequests++
      return {
        data: {
          code: 200,
          data: {
            content: (contents[path] ?? []).map((name) => ({ name, is_dir: false, size: 1 })),
            total: (contents[path] ?? []).length,
          },
        },
        status: 200,
        statusText: "OK",
        headers: {},
        config,
      }
    }

    await files.loadFolder("/dst")
    await files.loadFolder("/src")
    contents["/dst"].push("copied.txt")
    await files.refreshFiles("/src", ["/dst"])
    await files.loadPath("/dst")

    assert.equal(dstRequests, 2)
    assert.deepEqual(files.fileStore.files.map(({ name }: { name: string }) => name), [
      "before.txt",
      "copied.txt",
    ])
  } finally {
    await server.close()
    globals.forEach((key, index) => {
      if (descriptors[index])
        Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})
