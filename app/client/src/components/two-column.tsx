import type { ReactNode } from "react"

/**
 * A record's page: what the record *is* on the left, what it *belongs to* on
 * the right. One column on a narrow screen, in the order they are written.
 */
export function TwoColumnPage({
  main,
  side,
}: {
  main: ReactNode
  side?: ReactNode
}) {
  return (
    <div className="flex flex-col gap-4 xl:flex-row xl:items-start">
      <div className="flex min-w-0 flex-1 flex-col gap-4">{main}</div>
      {side ? (
        <div className="flex w-full flex-col gap-4 xl:w-[380px] xl:shrink-0">
          {side}
        </div>
      ) : null}
    </div>
  )
}
