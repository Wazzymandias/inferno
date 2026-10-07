# Build, test, and check the gateway service.
mod gateway 'services/gateway/Justfile'

# Run the web application's checks, including browser-to-model verification.
mod web 'apps/web/Justfile'

# Import the deployment entry point so `just deploy --dev` accepts flags.
import 'tools/deploy/Justfile'
