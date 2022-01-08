Feature: Build
  Scenario: Build with a build file
    When a file named "build.ninja" with:
    """
    """
    Then I successfully run `turtle`
