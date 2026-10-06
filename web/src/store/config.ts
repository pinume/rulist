import { createSignal } from "solid-js"
import { PublicConfig } from "~/types"

export const [config, setConfig] = createSignal<PublicConfig | null>(null)
export const loadConfig = (value: PublicConfig) => {
  setConfig(value)
  console.log(
    `%c Rulist %c ${value.version} %c https://github.com/pinume/Rulist`,
    "color: #fff; background: #5f5f5f",
    "color: #fff; background: #70c6be",
    "",
  )
}

export const logos = () => {
  const logo = config()?.logo.trim()
  if (!logo || logo === "favicon.ico") {
    return ["rulist.svg", "rulist-dark.svg"] as const
  }
  const [light, dark] = logo.split(/\r?\n/).map((item) => item.trim())
  return [light || "rulist.svg", dark || light || "rulist-dark.svg"] as const
}

export const logoUrl = (value: string) => {
  if (/^(?:https?:)?\/\//.test(value) || /^(?:data|blob):/.test(value)) {
    return value
  }
  return `/${value.replace(/^\/+/, "")}`
}

export const mainColor = () => config()?.main_color || "#1890ff"
