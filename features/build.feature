Feature: Build
  Scenario: Build with a build file
    When a file named "build.ninja" with:
    """
    """
    Then I successfully run `turtle`

  Scenario: Run a rule without inputs
    When a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello
    """
    Then I successfully run `turtle`
    And the stdout should contain exactly "hello"
