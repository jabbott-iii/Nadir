# The nadir-cook container image, ghcr.io/jabbott-iii/nadir-cook: the cook
# alone, linked statically, on nothing (ADR-0036). The release workflow
# builds it for linux/amd64 and linux/arm64 from the executables it built
# for each, which it puts in the build context as nadir-cook-amd64 and
# nadir-cook-arm64, beside the LICENSE. Nothing runs while it builds, so
# neither platform needs an emulator.
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

WORKDIR /work
ENTRYPOINT ["/nadir-cook"]
CMD ["--help"]

LABEL org.opencontainers.image.title="nadir-cook" \
      org.opencontainers.image.description="Offline asset pipeline of the Nadir game engine" \
      org.opencontainers.image.source="https://github.com/jabbott-iii/Nadir" \
      org.opencontainers.image.url="https://jabbott.dev/Nadir/" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}"
