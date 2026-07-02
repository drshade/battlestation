# battlestation — repo tasks. Run `make` (or `make help`) for the list.
#
# This is the single entrypoint. Targets are thin wrappers: they only call
# bin/* and stow/stow-*.sh — no logic lives here, so the scripts stay the
# source of truth and `make` stays a discoverable index of what you can do.

.DEFAULT_GOAL := help
.PHONY: help stow stow-root check fix drift

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*## "}{printf "  \033[1m%-11s\033[0m %s\n", $$1, $$2}'

stow: ## Stow home packages into $HOME (as you)
	@./stow/stow-home.sh

stow-root: ## Stow root packages into / (prompts for sudo)
	@sudo ./stow/stow-root.sh

check: ## Verify deployment + parse/lint — read-only (bin/doctor)
	@./bin/doctor

fix: ## Repair stow linkage by restowing (bin/doctor --fix)
	@./bin/doctor --fix

drift: ## Report machine config the repo doesn't manage (bin/drift)
	@./bin/drift
