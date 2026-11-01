# Tags a release of Nadir and pushes the tag, which starts the release
# workflow (.github/workflows/release.yml; docs/05-release.md, "Making a
# release"). For maintainers: Git is read-only for agents (AGENTS.md).
#
#     make release VERSION=v0.2.0             # tag HEAD and push the tag
#     make release VERSION=v0.2.0 DRY_RUN=1   # check, and print the rest
#
# The tag ruleset blocks moving or deleting a pushed tag, so it first
# checks what the workflow's gate will: that VERSION is `v` and HEAD's
# workspace version, and that HEAD is on the remote's main. The gate also
# needs CI to have passed on HEAD.

REMOTE ?= origin

# Runs a command and prints it, or with DRY_RUN only prints it, as written.
run = $(if $(DRY_RUN),echo "would run: $(1)",set -x; $(1))

.PHONY: release
release:
	@test -n '$(VERSION)' || { echo 'usage: make release VERSION=v<version> [REMOTE=origin] [DRY_RUN=1]' >&2; exit 1; }
	@version=$$(git show HEAD:Cargo.toml | sed -n '/^\[workspace\.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p'); \
	test '$(VERSION)' = "v$$version" || { echo "error: HEAD's workspace version is $$version, so its tag is v$$version, not $(VERSION)" >&2; exit 1; }
	@git fetch --quiet '$(REMOTE)' main
	@git merge-base --is-ancestor HEAD FETCH_HEAD || { echo "error: HEAD is not on $(REMOTE)'s main, and a release is made from main" >&2; exit 1; }
	@git ls-remote --exit-code --tags '$(REMOTE)' 'refs/tags/$(VERSION)' >/dev/null; \
	case $$? in \
	  0) echo 'error: $(REMOTE) already has the tag $(VERSION)' >&2; exit 1 ;; \
	  2) ;; \
	  *) exit 1 ;; \
	esac
	@if git rev-parse --quiet --verify 'refs/tags/$(VERSION)' >/dev/null; then \
	  test "$$(git rev-parse 'refs/tags/$(VERSION)^{commit}')" = "$$(git rev-parse HEAD)" || \
	    { echo 'error: $(VERSION) tags another commit here: remove it with git tag -d $(VERSION)' >&2; exit 1; }; \
	  echo '$(VERSION) already tags HEAD here'; \
	else \
	  $(call run,git tag -a '$(VERSION)' -m 'Nadir $(VERSION:v%=%)'); \
	fi
	@$(call run,git push '$(REMOTE)' 'refs/tags/$(VERSION)')
