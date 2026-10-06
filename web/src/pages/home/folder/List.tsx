import {
  IconButton,
  Divider,
  HStack,
  Icon,
  Menu,
  MenuContent,
  MenuGroup,
  MenuItem,
  MenuLabel,
  MenuTrigger,
  Text,
  Tooltip,
  useColorModeValue,
  VStack,
} from "@hope-ui/solid"
import { For, Show } from "solid-js"
import { useFiles, useRouter } from "~/hooks"
import {
  countMsg,
  directoryFilter,
  mainColor,
  OrderBy,
  fileStore,
  selectAll,
  selectedMsg,
  can,
  visibleFileIndexes,
} from "~/store"
import { Col, cols, ListItem } from "./ListItem"
import { bus } from "~/utils"
import { BsFilter } from "solid-icons/bs"
import { FiCheck } from "solid-icons/fi"
import { operations } from "../toolbar/operations"

const columnLabel: Record<string, string> = {
  name: "Name",
  size: "Size",
  modified: "Modified",
}

export const ListTitle = (props: {
  sortCallback: (orderBy: OrderBy, reverse: boolean) => void
  initialOrder: OrderBy
  initialReverse: boolean
}) => {
  const itemProps = (col: Col) => ({
    fontWeight: "semibold",
    fontSize: "$xs",
    color: "$neutral9",
    textTransform: "uppercase" as any,
    letterSpacing: "0.05em",
    textAlign: col.textAlign as any,
    cursor: "pointer",
    onClick: () => {
      if (col.name === props.initialOrder) {
        props.sortCallback(col.name as OrderBy, !props.initialReverse)
      } else {
        props.sortCallback(col.name as OrderBy, false)
      }
    },
  })

  return (
    <HStack
      class="title"
      w="$full"
      px="$3"
      py="$2"
      borderBottom="1px solid"
      borderColor="$neutral4"
      bgColor={useColorModeValue("$neutral2", "$neutral4")()}
      borderTopRadius="$xl"
    >
      <HStack w={cols[0].w} spacing="$1">
        <Text {...itemProps(cols[0])}>{columnLabel[cols[0].name]}</Text>
      </HStack>
      <Text
        w={cols[1].w}
        display={{ "@initial": "none", "@md": "inline" }}
        {...itemProps(cols[1])}
      >
        {columnLabel[cols[1].name]}
      </Text>
      <Text
        w={cols[2].w}
        {...itemProps(cols[2])}
        display={{ "@initial": "none", "@md": "inline" }}
      >
        {columnLabel[cols[2].name]}
      </Text>
      <HStack
        w={cols[3].w}
        minW={cols[3].minW}
        justifyContent="flex-end"
        flexShrink={0}
      >
        <Menu placement="bottom-end">
          <MenuTrigger
            px="$1_5"
            py="$1"
            h="auto"
            minW="unset"
            rounded="$md"
            cursor="pointer"
            bg="transparent"
            _hover={{ bgColor: useColorModeValue("$neutral3", "$neutral5")() }}
          >
            <HStack spacing="$1" alignItems="center">
              <Icon as={BsFilter} boxSize="$4" color="$neutral9" />
              <Text
                fontSize="$xs"
                fontWeight="semibold"
                color="$neutral9"
                display={{ "@initial": "none", "@md": "inline" }}
              >
                Sort
              </Text>
            </HStack>
          </MenuTrigger>
          <MenuContent minW="160px" shadow="$md" zIndex={100}>
            <MenuGroup>
              <MenuLabel fontSize="$xs" color="$neutral9" px="$3" py="$1">
                Sort by
              </MenuLabel>
              <MenuItem
                cursor="pointer"
                onSelect={() => props.sortCallback("name", fileStore.reverse)}
              >
                <HStack
                  w="$full"
                  justifyContent="space-between"
                  alignItems="center"
                >
                  <Text
                    fontSize="$sm"
                    color={
                      fileStore.orderBy === "name" ? mainColor() : undefined
                    }
                    fontWeight={
                      fileStore.orderBy === "name" ? "semibold" : "normal"
                    }
                  >
                    File name
                  </Text>
                  <Show when={fileStore.orderBy === "name"}>
                    <Icon as={FiCheck} color={mainColor()} />
                  </Show>
                </HStack>
              </MenuItem>
              <MenuItem
                cursor="pointer"
                onSelect={() =>
                  props.sortCallback("modified", fileStore.reverse)
                }
              >
                <HStack
                  w="$full"
                  justifyContent="space-between"
                  alignItems="center"
                >
                  <Text
                    fontSize="$sm"
                    color={
                      fileStore.orderBy === "modified" ? mainColor() : undefined
                    }
                    fontWeight={
                      fileStore.orderBy === "modified" ? "semibold" : "normal"
                    }
                  >
                    Modified
                  </Text>
                  <Show when={fileStore.orderBy === "modified"}>
                    <Icon as={FiCheck} color={mainColor()} />
                  </Show>
                </HStack>
              </MenuItem>
            </MenuGroup>
            <Divider my="$1" />
            <MenuGroup>
              <MenuLabel fontSize="$xs" color="$neutral9" px="$3" py="$1">
                Sort order
              </MenuLabel>
              <MenuItem
                cursor="pointer"
                onSelect={() => props.sortCallback(fileStore.orderBy, false)}
              >
                <HStack
                  w="$full"
                  justifyContent="space-between"
                  alignItems="center"
                >
                  <Text
                    fontSize="$sm"
                    color={!fileStore.reverse ? mainColor() : undefined}
                    fontWeight={!fileStore.reverse ? "semibold" : "normal"}
                  >
                    A to Z
                  </Text>
                  <Show when={!fileStore.reverse}>
                    <Icon as={FiCheck} color={mainColor()} />
                  </Show>
                </HStack>
              </MenuItem>
              <MenuItem
                cursor="pointer"
                onSelect={() => props.sortCallback(fileStore.orderBy, true)}
              >
                <HStack
                  w="$full"
                  justifyContent="space-between"
                  alignItems="center"
                >
                  <Text
                    fontSize="$sm"
                    color={fileStore.reverse ? mainColor() : undefined}
                    fontWeight={fileStore.reverse ? "semibold" : "normal"}
                  >
                    Z to A
                  </Text>
                  <Show when={fileStore.reverse}>
                    <Icon as={FiCheck} color={mainColor()} />
                  </Show>
                </HStack>
              </MenuItem>
            </MenuGroup>
          </MenuContent>
        </Menu>
      </HStack>
    </HStack>
  )
}

const ListLayout = () => {
  const { sort } = useFiles()
  const { pathname } = useRouter()

  return (
    <VStack
      class="list"
      w="$full"
      spacing="$0"
      onWheel={(event: WheelEvent) => {
        if (!event.shiftKey || event.ctrlKey || event.metaKey) return
        event.preventDefault()
        const scale =
          event.deltaMode === 1
            ? 16
            : event.deltaMode === 2
              ? window.innerHeight
              : 1
        window.scrollBy({
          top: (event.deltaY || event.deltaX) * scale,
          behavior: "instant",
        })
      }}
    >
      <VStack
        class="list-heading"
        w="$full"
        spacing="$0"
        position="sticky"
        top={pathname().split("/").filter(Boolean).length ? "100px" : "60px"}
        zIndex={80}
      >
        <ListTitle
          sortCallback={(orderBy, reverse) => {
            void sort(orderBy, reverse)
          }}
          initialOrder={fileStore.orderBy}
          initialReverse={fileStore.reverse}
        />
        <Show when={selectedMsg()}>
          <HStack
            class="selection-actions"
            w="$full"
            px={{ "@initial": "$3", "@md": "$4" }}
            py="$2"
            spacing="$3"
            borderBottom="1px solid"
            borderColor="$neutral4"
            bgColor={useColorModeValue("$info2", "$neutral4")()}
          >
            <For
              each={
                [
                  { key: "copy", label: "Copy" },
                  { key: "move", label: "Move" },
                  { key: "delete", label: "Delete" },
                  { key: "cancel_select", label: "Cancel selection" },
                ] as const
              }
            >
              {(action) => (
                <Show when={action.key === "cancel_select" || can(action.key)}>
                  <Tooltip label={action.label} withArrow>
                    <IconButton
                      size="sm"
                      variant="ghost"
                      aria-label={action.label}
                      color={operations[action.key].color ?? "$neutral10"}
                      icon={
                        <Icon as={operations[action.key].icon} boxSize="$5" />
                      }
                      onClick={() =>
                        action.key === "cancel_select"
                          ? selectAll(false)
                          : bus.emit("tool", action.key)
                      }
                    />
                  </Tooltip>
                </Show>
              )}
            </For>
          </HStack>
        </Show>
      </VStack>
      <For each={visibleFileIndexes()}>
        {(index) => <ListItem obj={fileStore.files[index]} index={index} />}
      </For>
      <Show
        when={directoryFilter().trim() && visibleFileIndexes().length === 0}
      >
        <Text size="sm" color="$neutral11" p="$4">
          No matching files on this page
        </Text>
      </Show>
      <HStack
        w="$full"
        px="$3"
        py="$2"
        borderTop="1px solid"
        borderColor="$neutral4"
        bgColor={useColorModeValue("$neutral1", "$neutral3")()}
        justifyContent="space-between"
        alignItems="center"
        position="sticky"
        bottom={0}
        zIndex={80}
        borderBottomRadius="$xl"
        shadow="$sm"
      >
        <Text
          class="selection-summary"
          size="xs"
          color="$neutral10"
          aria-live="polite"
        >
          {selectedMsg() ||
            (directoryFilter().trim()
              ? `${visibleFileIndexes().length} matches on this page`
              : countMsg())}
        </Text>
      </HStack>
    </VStack>
  )
}

export default ListLayout
