// Swift smoke test of the generated bindings (AZ-4007 AC-2), run only on the owner Mac
// (vault-core/ci/vault-core-ci.sh swift); compiled into the same module as the generated vault_core.swift.

// Arrange
let rolledBack = VaultError.HeaderRolledBack(seen: 1, pinned: 2)
let bound = VaultError.ParamsOutOfBounds(field: .tCost)

// Act: the first call loads the library and runs the UniFFI contract-version check; each round trip
// allocates and frees a RustBuffer inside the Rust library.
uniffiEnsureVaultCoreInitialized()
let rolledBackBack = try FfiConverterTypeVaultError_lift(FfiConverterTypeVaultError_lower(rolledBack))
let boundBack = try FfiConverterTypeVaultError_lift(FfiConverterTypeVaultError_lower(bound))

// Assert
guard rolledBackBack == rolledBack else { fatalError("HeaderRolledBack did not round-trip: \(rolledBackBack)") }
guard boundBack == bound else { fatalError("ParamsOutOfBounds did not round-trip: \(boundBack)") }
print("[vault-uniffi-swift] PASS: library loaded, contract version checked, VaultError round trip")
