# Copyright 2026 Joseph Anthony Abbott III
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# The nadir-cook container image, ghcr.io/jabbott-iii/nadir-cook: the cook
# alone, linked statically, on nothing (ADR-0036). The release workflow
# builds it for linux/amd64 and linux/arm64 from the executables it built
# for each, which it puts in the build context as nadir-cook-amd64 and
# nadir-cook-arm64, beside the LICENSE and NOTICE. Nothing runs while it
# builds, so neither platform needs an emulator.
#
# Cook the folder `assets` into `cooked`, as the user who owns them:
#
#     docker run --rm --user "$(id -u):$(id -g)" --volume "$PWD:/work" \
#         ghcr.io/jabbott-iii/nadir-cook assets cooked
FROM scratch

ARG TARGETARCH
ARG VERSION=unknown

COPY nadir-cook-${TARGETARCH} /nadir-cook
COPY LICENSE /LICENSE
COPY NOTICE /NOTICE

WORKDIR /work
ENTRYPOINT ["/nadir-cook"]
CMD ["--help"]

LABEL org.opencontainers.image.title="nadir-cook" \
      org.opencontainers.image.description="Offline asset pipeline of the Nadir game engine" \
      org.opencontainers.image.source="https://github.com/jabbott-iii/Nadir" \
      org.opencontainers.image.url="https://jabbott.dev/Nadir/" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}"
