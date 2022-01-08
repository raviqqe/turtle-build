Feature: Build
  Scenario: Build with a build file
    Given a file named "build.ninja" with:
    """
    """
    When I run `turtle`
    Then the exit status should be 0
