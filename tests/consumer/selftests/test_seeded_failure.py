def test_seeded_failure_fails_the_run():
    # Arrange
    expected = "the run fails"

    # Act
    actual = "the run passes"

    # Assert
    assert actual == expected, "seeded failure (SEED_FAIL=1): this test must turn the run red"
