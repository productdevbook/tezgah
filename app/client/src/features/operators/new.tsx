import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"

import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useT } from "@/panel/i18n"
import {
  createOperator,
  newOperator,
  ROLE_MEANS,
  type NewOperator,
  type Role,
} from "@/features/operators/api"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export function NewOperatorForm() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal onClose={() => void navigate({ to: "/operators" })}>
      <Body />
    </RouteFocusModal>
  )
}

function Body() {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<NewOperator>({
    resolver: zodResolver(newOperator),
    defaultValues: { email: "", name: "", password: "", role: "staff" },
  })

  const mutation = useMutation({
    mutationFn: (body: NewOperator) => createOperator(body),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["operators"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) => mutation.mutateAsync(values)}
    >
      <RouteFocusModal.Header
        title={t("form.operator.title")}
        description={t("form.operator.why")}
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="name" label={t("field.name")}>
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="email"
            label={t("field.email")}
          >
            {(field) => <Input id={field.name} type="email" {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="role"
            label={t("field.role")}
            description={t("form.operator.roleWhy")}
          >
            {(field) => (
              <Select
                value={field.value}
                onValueChange={(value) => field.onChange(value)}
              >
                <SelectTrigger id={field.name}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(Object.keys(ROLE_MEANS) as Role[]).map((option) => (
                    <SelectItem key={option} value={option}>
                      {option} — {ROLE_MEANS[option]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </FormField>
          <FormField
            control={form.control}
            name="password"
            label={t("field.password")}
            description={t("form.operator.passwordWhy")}
          >
            {(field) => (
              <Input
                id={field.name}
                type="password"
                autoComplete="new-password"
                {...field}
              />
            )}
          </FormField>
        </div>
      </RouteFocusModal.Body>
      <RouteFocusModal.Footer>
        <Button type="button" variant="outline" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button type="submit" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting
            ? t("actions.saving")
            : t("actions.create")}
        </Button>
      </RouteFocusModal.Footer>
    </RouteModalForm>
  )
}
