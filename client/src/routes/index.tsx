import { createFileRoute } from "@tanstack/react-router"

import { Overview } from "@/features/overview/screen"

export const Route = createFileRoute("/")({ component: Overview })
