# Release and publication

The integrated source is on the default `Main` branch. [TRANSFORMATION.md](TRANSFORMATION.md) records the current engineering work, measured results and verification scope.

1. Pull `Main` and confirm the native quality workflow passes on Linux, Windows and macOS. Reproduce the locked release checks from [BUILDING.md](BUILDING.md).
2. Run **Build release packages** manually to collect desktop, Android development, unsigned iOS and corresponding-source artifacts. Inspect each job and its artifacts before distributing a platform build.
3. Extract packages, verify checksums and licenses, then test installation, audio output, microphone, dialogs, secure storage, media controls and intended integrations on the supported platforms. Follow [PLATFORMS.md](PLATFORMS.md) for outstanding device validation.
4. Configure the owner's signing identities privately. Windows distribution needs the intended code-signing identity; macOS distribution needs Developer ID signing and notarization; Android updates need a retained production key; iOS devices need valid signing and provisioning.
5. For an approved release, update the package version and create a new version tag. Preserve existing tags. The tag workflow creates a reviewable draft GitHub release after its required jobs pass.
6. Review the draft's platform scope, checksums, notices and corresponding source before publication. State any unsigned or development-only artifacts clearly.

The Linux package checks extract and execute artifacts; they do not establish clean-system installation or physical-device readiness. Current responsive phone captures come from the Linux renderer. Signing credentials, private media and account secrets are excluded from the repository.
