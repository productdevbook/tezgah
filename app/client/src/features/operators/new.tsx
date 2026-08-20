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
  type NewOperator,
} from "@/features/operators/api"

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
    defaultValues: { email: "", name: "", password: "" },
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
        title="New operator"
        description="The password is set here and shown to nobody afterwards. There is no invitation e-mail: this server has no mailer."
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="name" label="Name">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField control={form.control} name="email" label="E-mail">
            {(field) => <Input id={field.name} type="email" {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="password"
            label="Password"
            description="Twelve characters at least. Tell them out of band — nothing here can send it."
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
