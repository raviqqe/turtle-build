Feature: Error

  Scenario: Fail to read a root build file
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "build.ninja"

  Scenario: Fail to read a included build file
    Given a file named "build.ninja" with:
      """
      include foo.ninja

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "foo.ninja"

  Scenario: Fail to read a child build file
    Given a file named "build.ninja" with:
      """
      include foo.ninja

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "foo.ninja"
