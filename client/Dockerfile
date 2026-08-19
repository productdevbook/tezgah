# syntax=docker/dockerfile:1

# canary, not a release: bun.lock's format tracks the bun version that wrote
# it, and .github/workflows/ci.yml pins the same canary for the same reason —
# a release build cannot read a lockfile a canary wrote.
FROM oven/bun:canary-alpine AS build
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build

# Unprivileged: listens on 8080 and runs as uid 101 out of the box, so the
# container needs no USER/port surgery of its own.
FROM nginxinc/nginx-unprivileged:1.27-alpine

COPY --from=build /app/dist /usr/share/nginx/html
COPY nginx.conf.template /etc/nginx/templates/default.conf.template

# Restricts the image's own envsubst step to variables this template actually
# uses, so an nginx directive that happens to share a name with something in
# the container's environment (nginx has plenty of $host, $uri, ...) is never
# rewritten by accident.
ENV NGINX_ENVSUBST_FILTER=^TEZGAH_
ENV TEZGAH_API_UPSTREAM=tezgah-server:8080

EXPOSE 8080
