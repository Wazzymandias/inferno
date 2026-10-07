# Build, test, and check the gateway service.
mod gateway 'services/gateway/Justfile'

# Import the deployment entry point so `just deploy --dev` accepts flags.
import 'tools/deploy/Justfile'
