// Run: node web/tests/optimization.browser.mjs <rulist binary> <Chromium executable>
import assert from "node:assert/strict"
import { spawn, spawnSync } from "node:child_process"
import {
  existsSync,
  readFileSync,
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
} from "node:fs"
import { tmpdir } from "node:os"
import { createServer as createNetServer } from "node:net"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

const [binary, chromium] = process.argv.slice(2)
assert.ok(binary && chromium, "Pass the Rulist binary and Chromium executable")
const tmp = mkdtempSync(resolve(tmpdir(), "rulist-browser-"))
const root = resolve(tmp, "files")
mkdirSync(root)
mkdirSync(resolve(root, "nested"))
writeFileSync(resolve(root, "nested", "child.txt"), "nested fixture")
for (let index = 0; index < 80; index++) {
  writeFileSync(resolve(root, `row-${String(index).padStart(3, "0")}.txt`), "row")
}
const text = "中文 preview\n".repeat(10000)
for (const [name, content] of Object.entries({
  "large.txt": text,
  "small.txt": "first\nsecond\n",
  "wide.txt": "x".repeat(256 * 1024 + 1),
  "report.docx": "office fixture",
  "a.txt": "archive a",
  "b.txt": "archive b",
  "c.txt": "archive c",
  "d.txt": "archive d",
}))
  writeFileSync(resolve(root, name), content)
const env = { ...process.env, HOME: root }
const init = spawnSync(
  resolve(binary),
  ["--data-dir", resolve(tmp, "data"), "interactive"],
  {
    input: "0\n",
    encoding: "utf8",
    env,
  },
)
assert.equal(init.status, 0, init.stderr)
const password = init.stdout.match(/Password: (\S+)/)[1]
const portServer = createNetServer()
await new Promise((r) => portServer.listen(0, "127.0.0.1", r))
const backendPort = portServer.address().port
await new Promise((r) => portServer.close(r))
const backend = spawn(
  resolve(binary),
  ["--data-dir", resolve(tmp, "data"), "server", "--port", String(backendPort)],
  { env },
)
let vite, browser, socket
const pending = new Map()
let id = 0
const events = []
const delay = (ms) => new Promise((r) => setTimeout(r, ms))
async function until(fn, timeout = 15000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const result = await fn()
    if (result) return result
    await delay(50)
  }
  throw new Error(`Timed out waiting for browser state: ${fn}`)
}
try {
  let output = ""
  backend.stdout.on("data", (chunk) => {
    output += chunk
  })
  const address = await until(
    () => output.match(/start HTTP server @ (127\.0\.0\.1:\d+)/)?.[1],
  )
  vite = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    cacheDir: resolve(tmp, "vite-cache"),
    server: {
      host: "127.0.0.1",
      port: 0,
      proxy: Object.fromEntries(
        ["/api", "/d", "/p"].map((path) => [
          path,
          { target: `http://${address}` },
        ]),
      ),
    },
    logLevel: "silent",
  })
  await vite.listen()
  const base = `http://127.0.0.1:${vite.httpServer.address().port}`
  browser = spawn(chromium, [
    "--no-sandbox",
    "--headless",
    "--remote-debugging-port=0",
    `--user-data-dir=${tmp}/browser`,
    "about:blank",
  ])
  let browserOutput = ""
  browser.stderr.on("data", (chunk) => {
    browserOutput += chunk
  })
  const ws = await until(
    () => browserOutput.match(/DevTools listening on (ws:\/\/[^\s]+)/)?.[1],
  )
  const debugBase = new URL(ws)
  debugBase.protocol = "http:"
  const targets = await (await fetch(`${debugBase.origin}/json/list`)).json()
  socket = new WebSocket(
    targets.find((t) => t.type === "page").webSocketDebuggerUrl,
  )
  await new Promise((r) => socket.addEventListener("open", r, { once: true }))
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data)
    if (message.id) {
      const handlers = pending.get(message.id)
      pending.delete(message.id)
      if (message.error)
        handlers.reject(new Error(JSON.stringify(message.error)))
      else handlers.resolve(message.result)
    } else events.push(message)
  })
  const cdp = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const requestId = ++id
      pending.set(requestId, { resolve, reject })
      socket.send(JSON.stringify({ id: requestId, method, params }))
    })
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    })
    if (result.exceptionDetails)
      throw new Error(JSON.stringify(result.exceptionDetails))
    return result.result.value
  }
  const clickFile = async (name, modifiers = 0) => {
    const point = await evaluate(`(() => {
      const row = [...document.querySelectorAll('.list-item')].find(item => item.querySelector('.name')?.textContent === ${JSON.stringify(name)})
      if (!row) return null
      row.scrollIntoView({ block: 'center' })
      const rect = row.getBoundingClientRect()
      return { x: rect.left + 4, y: rect.top + rect.height / 2 }
    })()`)
    assert.ok(point, `File row not found: ${name}`)
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: point.x,
      y: point.y,
      modifiers,
    })
    await cdp("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: point.x,
      y: point.y,
      button: "left",
      clickCount: 1,
      modifiers,
    })
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: point.x,
      y: point.y,
      button: "left",
      clickCount: 1,
      modifiers,
    })
  }
  const clickButton = async (name) => {
    const point = await evaluate(`(() => {
      const button = [...document.querySelectorAll('button')].find(item => item.textContent.trim() === ${JSON.stringify(name)})
      if (!button) return null
      const rect = button.getBoundingClientRect()
      return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
    })()`)
    assert.ok(point, `Button not found: ${name}`)
    await cdp("Input.dispatchMouseEvent", { type: "mouseMoved", ...point })
    await cdp("Input.dispatchMouseEvent", {
      type: "mousePressed",
      ...point,
      button: "left",
      clickCount: 1,
    })
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      ...point,
      button: "left",
      clickCount: 1,
    })
  }
  await cdp("Runtime.enable")
  await cdp("Network.enable")
  await cdp("Page.enable")
  await cdp("Browser.setDownloadBehavior", {
    behavior: "allow",
    downloadPath: tmp,
  })
  await cdp("Browser.grantPermissions", {
    origin: base,
    permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"],
  })
  await cdp("Page.navigate", { url: `${base}/@login` })
  await until(() => evaluate("document.querySelectorAll('input').length >= 2"))
  await until(() => evaluate("[...document.querySelectorAll('img')].some(image => image.naturalWidth > 0)"))
  await evaluate(`(() => {
    const inputs = document.querySelectorAll('input')
    inputs[0].value = 'admin'; inputs[1].value = ${JSON.stringify(password)}
    for (const input of [inputs[0], inputs[1]]) input.dispatchEvent(new Event('input', { bubbles: true }))
    Array.from(document.querySelectorAll('button')).find(b => b.textContent.includes('Sign in')).click()
  })()`)
  await until(() =>
    evaluate(
      "localStorage.getItem('token') && document.body?.textContent?.includes('large.txt')",
    ),
  )
  assert.ok(await evaluate("document.querySelector('.header-left img')?.naturalWidth > 0"), "Logo loads at root")
  await cdp("Page.navigate", { url: `${base}/nested/` })
  await until(() => evaluate("!!document.querySelector('.header-left img')"))
  await until(() => evaluate("document.querySelector('.header-left img')?.naturalWidth > 0"))
  await cdp("Page.reload")
  await until(() => evaluate("document.querySelector('.header-left img')?.naturalWidth > 0"))
  console.log("logo: root, nested route and nested refresh passed")
  await cdp("Page.navigate", { url: `${base}/` })
  await until(() => evaluate("document.body?.textContent?.includes('row-079.txt')"))
  await clickFile("row-000.txt", 2)
  await until(() => evaluate("document.querySelector('.list-item.selected .name')?.textContent === 'row-000.txt'"))
  await cdp("Input.dispatchMouseEvent", {
    type: "mouseWheel",
    x: 400,
    y: 400,
    deltaX: 0,
    deltaY: 1800,
    modifiers: 8,
  })
  await until(() => evaluate("window.scrollY > 0"))
  await clickFile("row-079.txt", 8)
  await until(() => evaluate("document.querySelector('.selection-summary')?.textContent.includes('80 files')"))
  const selectionActions = await evaluate("[...document.querySelectorAll('.selection-actions button')].map(button => button.getAttribute('aria-label'))")
  assert.deepEqual(selectionActions, ["Copy", "Move", "Delete", "Cancel selection"])
  const stickySummary = await evaluate(`(() => {
    const rect = document.querySelector('.selection-summary').getBoundingClientRect()
    return rect.top >= 0 && rect.bottom <= innerHeight
  })()`)
  assert.equal(stickySummary, true, "Selection count stays visible while scrolling")
  await clickFile("row-020.txt", 8)
  await until(() => evaluate("document.querySelector('.selection-summary')?.textContent.includes('21 files')"))
  console.log("selection: Shift+wheel range expansion, sticky count, and range shrink passed")
  await cdp("Page.navigate", { url: `${base}/` })
  await until(() => evaluate("document.body?.textContent?.includes('large.txt')"))
  for (const [name, count, expected] of [
    ["large.txt", 1, text],
    ["wide.txt", 1, "x".repeat(256 * 1024 + 1)],
    ["small.txt", 3, null],
  ]) {
    await cdp("Page.navigate", { url: `${base}/${name}` })
    await until(() =>
      evaluate("document.body?.textContent?.includes('Lines:')"),
    )
    const result = await evaluate(
      `({ count: document.querySelectorAll('pre').length, text: document.querySelector('pre').textContent, dom: document.querySelectorAll('*').length })`,
    )
    assert.equal(result.count, count, name)
    if (expected !== null) {
      assert.equal(result.text, expected)
      await clickButton("Copy")
      await until(() =>
        evaluate("document.body?.textContent?.includes('Copied')"),
      )
      assert.equal(await evaluate("navigator.clipboard.readText()"), expected)
    }
    await evaluate(
      "Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'Wrap').click()",
    )
    assert.equal(
      await evaluate(
        "getComputedStyle(document.querySelector('pre')).whiteSpace",
      ),
      "pre-wrap",
    )
    console.log(
      `${name}: ${result.count} pre elements, ${result.dom} DOM elements`,
    )
  }
  events.length = 0
  await evaluate(
    "(async () => { const files = await import('/src/store/files.ts'); files.setShouldKeepState(true); const { bus } = await import('/src/utils/bus.ts'); bus.emit('to', '/report.docx') })()",
  )
  await until(() =>
    evaluate(
      "document.body?.textContent?.includes('report.docx') && document.body?.textContent?.includes('Download')",
    ),
  )
  assert.equal(
    events.some(
      (e) =>
        e.method === "Network.requestWillBeSent" &&
        e.params.request.url.includes("/api/fs/preview"),
    ),
    false,
  )
  await evaluate(
    "Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'Download').click()",
  )
  await until(() => existsSync(resolve(tmp, "report.docx")))
  assert.equal(
    readFileSync(resolve(tmp, "report.docx"), "utf8"),
    "office fixture",
  )
  await evaluate(
    "(async () => { const files = await import('/src/store/files.ts'); files.setShouldKeepState(false); const { bus } = await import('/src/utils/bus.ts'); bus.emit('to', '/small.txt') })()",
  )
  await until(() => evaluate("document.body?.textContent?.includes('Lines:')"))

  // Keep the real archive UI and ZIP writer; replace only the disk sink for assertions.
  await cdp("Page.navigate", { url: `${base}/` })
  await until(() =>
    evaluate("document.body?.textContent?.includes('large.txt')"),
  )
  await evaluate(`(async () => {
    window.unhandled = []
    window.addEventListener('unhandledrejection', e => window.unhandled.push(String(e.reason)))
    window.archiveMode = 'success'; window.archiveChunks = []; window.aborted = 0; window.started = 0
    await import('/src/pages/home/toolbar/PackageDownload.tsx')
    const resource = performance.getEntriesByType('resource').find(r => r.name.includes('/streamsaver.js'))
    const saver = (await import(resource.name)).default
    saver.createWriteStream = () => new WritableStream({ write(chunk) {
      if (window.archiveMode === 'sink') throw new Error('sink failed')
      window.archiveChunks.push(Array.from(chunk))
    }})
    const originalFetch = window.fetch.bind(window)
    window.fetch = async (url, options) => {
      if (String(url).includes('/d/') && ['network', 'pending'].includes(window.archiveMode)) {
        if (window.archiveMode === 'network' && String(url).includes('/b.txt')) throw new Error('prefetch failed')
        window.started++
        return new Promise((resolve, reject) => options.signal.addEventListener('abort', () => {
          window.aborted++; reject(options.signal.reason)
        }, { once: true }))
      }
      if (String(url).includes('/d/') && window.archiveMode === 'read') {
        return new Response(new ReadableStream({ pull(ctrl) { ctrl.error(new Error('read failed')) } }))
      }
      return originalFetch(url, options)
    }
    window.startArchive = async (mode) => {
      window.archiveMode = mode; window.archiveChunks = []; window.aborted = 0; window.started = 0
      const files = await import('/src/store/files.ts')
      files.selectAll(false)
      for (const name of ['a.txt', 'b.txt', 'c.txt', 'd.txt']) {
        files.selectIndex(files.fileStore.files.findIndex(f => f.name === name), true)
      }
      const { bus } = await import('/src/utils/bus.ts')
      bus.emit('tool', 'package_download_direct')
    }
  })()`)
  for (const mode of ["success", "network", "read", "sink"]) {
    await evaluate(`window.startArchive(${JSON.stringify(mode)})`)
    await until(() =>
      evaluate(
        `document.body?.textContent?.includes(${JSON.stringify(mode === "success" ? "Download complete" : "Archive download failed")})`,
      ),
    )
    assert.deepEqual(await evaluate("window.unhandled"), [])
    if (mode === "network") {
      await until(() => evaluate("window.aborted > 0"))
      assert.ok((await evaluate("window.aborted")) > 0)
    }
    if (mode === "success") {
      const chunks = await evaluate("window.archiveChunks")
      writeFileSync(
        resolve(tmp, "archive.zip"),
        Buffer.concat(chunks.map((c) => Buffer.from(c))),
      )
      const zip = spawnSync(
        "python3",
        [
          "-c",
          "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; assert set(z.namelist())=={'a.txt','b.txt','c.txt','d.txt'}; assert z.read('a.txt')==b'archive a'",
          resolve(tmp, "archive.zip"),
        ],
        { encoding: "utf8" },
      )
      assert.equal(zip.status, 0, zip.stderr)
    }
    await evaluate(
      "Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'Close').click()",
    )
    await until(() =>
      evaluate("!document.body?.textContent?.includes('Download as archive')"),
    )
    console.log(`archive ${mode}: passed`)
  }
  // Disposing the archive during pending network reads must cancel them too.
  await evaluate("window.startArchive('pending')")
  await until(() => evaluate("window.started > 0"))
  await evaluate(
    "(async () => { const { bus } = await import('/src/utils/bus.ts'); bus.emit('to', '/@login') })()",
  )
  await until(() =>
    evaluate(
      "document.body?.textContent?.includes('Sign in') && window.aborted > 0",
    ),
  )
  assert.deepEqual(await evaluate("window.unhandled"), [])
  console.log("browser optimization checks passed")
} finally {
  socket?.close()
  browser?.kill()
  backend.kill()
  await vite?.close()
  await Promise.all(
    [browser, backend]
      .filter(Boolean)
      .map((p) =>
        p.exitCode === null ? new Promise((r) => p.once("exit", r)) : undefined,
      ),
  )
  rmSync(tmp, { recursive: true, force: true })
}
