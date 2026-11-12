// JVM smoke test of the generated Kotlin bindings (AZ-4007 AC-2). Compiled together with the generated
// vault_core.kt, so it may use the module-internal library objects; run by vault-core/ci/vault-core-ci.sh uniffi.
package uniffi.vault_core

private fun roundTrip(sent: VaultException): VaultException =
    FfiConverterTypeVaultError.lift(FfiConverterTypeVaultError.lower(sent))

fun main() {
    // Arrange
    val rolledBack = VaultException.HeaderRolledBack(1uL, 2uL)
    val bound = VaultException.ParamsOutOfBounds(ParamField.T_COST)

    // Act: the first call loads the native library and runs the UniFFI contract-version check; each round
    // trip allocates and frees a RustBuffer inside the Rust library.
    IntegrityCheckingUniffiLib.ensureInitialized()
    val rolledBackBack = roundTrip(rolledBack)
    val boundBack = roundTrip(bound)

    // Assert
    check(rolledBackBack is VaultException.HeaderRolledBack && rolledBackBack.seen == 1uL && rolledBackBack.pinned == 2uL) {
        "HeaderRolledBack did not round-trip: $rolledBackBack"
    }
    check(boundBack is VaultException.ParamsOutOfBounds && boundBack.`field` == ParamField.T_COST) {
        "ParamsOutOfBounds did not round-trip: $boundBack"
    }
    println("[vault-uniffi-kotlin] PASS: library loaded, contract version checked, VaultError round trip")
}
