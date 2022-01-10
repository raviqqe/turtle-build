Feature: Error
  Scenario: Build nothing
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "build.ninja"
