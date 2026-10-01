# Project Conventions

- Follow the twelve-factor app methodology for every change: declare and lock
  dependencies, keep deploy-specific configuration in environment variables,
  treat backing services as replaceable resources, and separate build/release/run.
- Keep processes stateless and independently scalable, bind the HTTP service to
  a configurable port, and keep development and production behavior aligned.
- Write runtime events to stdout; let the execution environment collect and route
  logs. Never log credentials, request bodies, or sensitive URL query parameters.
- Run any future administrative jobs as one-off commands using the same release
  and configuration. Do not invent administrative commands without a real task.
- Keep secrets and local `.env` files out of version control. Document new settings
  in `.env.example` and README, and pass them through Compose when applicable.
