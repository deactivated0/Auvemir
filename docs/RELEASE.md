# Release and publication

The repository is ready for GitHub source review. Publish the source under the name **Auvemir**. Run the CI and native release workflows before representing additional platforms as supported.

1. Connect the GitHub account and select the repository owner.
2. Create the Auvemir repository and push the reviewed source and Cargo.lock.
3. Run “Build release packages” manually to collect native desktop artifacts.
4. Inspect logs and artifacts. Platform failures must be fixed before release.
5. Tag v0.1.0 only when the intended development release is approved. Tag builds create a draft GitHub release with corresponding source.
6. Review platform-status and feature-gap documentation, attach checksums and the source bundle, then publish the development release.

The initial delivery is not a completed implementation of the full original brief. Android/iOS, casting and the remaining integration/features must be finished and validated separately. GitHub login does not itself implement those features. macOS builds use ad-hoc signing, not a Developer ID/notarized identity; Windows installers have no fabricated signing certificate.
