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

  @turtle
  Scenario: Fail to include a build file in itself
    Given a file named "build.ninja" with:
      """
      include build.ninja

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "build file dependency cycle detected"

  @turtle
  Scenario: Fail to include build files in each other
    Given a file named "build.ninja" with:
      """
      include foo.ninja

      """
    And a file named "foo.ninja" with:
      """
      include build.ninja

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "build file dependency cycle detected"
    And the stderr should contain "foo.ninja"

  @turtle
  Scenario: Fail to include child build files in each other
    Given a file named "build.ninja" with:
      """
      subninja foo.ninja

      """
    And a file named "foo.ninja" with:
      """
      subninja build.ninja

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "build file dependency cycle detected"
