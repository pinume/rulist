import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbSeparator,
  HStack,
  useColorModeValue,
} from "@hope-ui/solid"
import { Link } from "@solidjs/router"
import { createMemo, For, Show } from "solid-js"
import { hairlineBorderColor } from "~/app/theme"
import { useFiles, useRouter } from "~/hooks"
import { encodePath, pathJoin } from "~/utils"

export const Nav = () => {
  const { pathname } = useRouter()
  const paths = createMemo(() => {
    return pathname().split("/").filter(Boolean)
  })
  const { rememberDirectory } = useFiles()

  return (
    <Show when={paths().length > 0}>
      <HStack
        position="sticky"
        top="50px"
        zIndex={90}
        minH="36px"
        py="$1"
        px={{ "@initial": "$2", "@md": "$3" }}
        rounded="$lg"
        bgColor={useColorModeValue(
          "rgba(248, 250, 252, 0.82)",
          "rgba(9, 9, 11, 0.82)",
        )()}
        css={{
          backdropFilter: "blur(12px)",
          WebkitBackdropFilter: "blur(12px)",
        }}
        border="1px solid"
        borderColor={hairlineBorderColor()}
        w="$full"
      >
        <Breadcrumb class="nav" minW={0} overflow="hidden">
          <BreadcrumbItem class="nav-item">
            <BreadcrumbLink
              class="nav-link"
              as={Link}
              href={pathJoin("/")}
              color="$neutral10"
              _hover={{ color: "$neutral12" }}
              cursor="pointer"
              px="$1"
              py="$0_5"
              onMouseEnter={() => rememberDirectory("/")}
            >
              Files
            </BreadcrumbLink>
            <BreadcrumbSeparator class="nav-separator" />
          </BreadcrumbItem>
          <For each={paths()}>
            {(name, i) => {
              const isLast = createMemo(() => i() === paths().length - 1)
              const path = `/${paths()
                .slice(0, i() + 1)
                .join("/")}`
              const href = encodePath(path)
              return (
                <BreadcrumbItem class="nav-item">
                  <BreadcrumbLink
                    class="nav-link"
                    css={{
                      wordBreak: "break-all",
                    }}
                    color={isLast() ? "$neutral12" : "$neutral10"}
                    _hover={{ color: "$neutral12" }}
                    cursor="pointer"
                    px="$1"
                    py="$0_5"
                    currentPage={isLast()}
                    as={isLast() ? undefined : Link}
                    href={pathJoin(href)}
                    onMouseEnter={() => rememberDirectory(path)}
                  >
                    {name}
                  </BreadcrumbLink>
                  <Show when={!isLast()}>
                    <BreadcrumbSeparator class="nav-separator" />
                  </Show>
                </BreadcrumbItem>
              )
            }}
          </For>
        </Breadcrumb>
      </HStack>
    </Show>
  )
}
