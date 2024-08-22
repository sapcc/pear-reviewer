MAKEFLAGS=--warn-undefined-variables
# /bin/sh is dash on Debian which does not support all features of ash/bash
# to fix that we use /bin/bash only on Debian to not break Alpine
ifneq (,$(wildcard /etc/os-release)) # check file existence
	ifneq ($(shell grep -c debian /etc/os-release),0)
		SHELL := /bin/bash
	endif
endif

prepare-static-check: FORCE
	@if ! hash addlicense 2>/dev/null; then  printf "\e[1;36m>> Installing addlicense...\e[0m\n";  go install github.com/google/addlicense@latest; fi

license-headers: FORCE prepare-static-check
	@printf "\e[1;36m>> addlicense\e[0m\n"
	@addlicense -c "SAP SE" -- $(shell find -name *.rs)

check-license-headers: FORCE prepare-static-check
	@printf "\e[1;36m>> addlicense --check\e[0m\n"
	@addlicense --check -- $(shell find -name *.rs)

clean: FORCE
	git clean -dxf build

.PHONY: FORCE
