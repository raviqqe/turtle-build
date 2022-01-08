Feature: Build
  Scenario: Build with a build file
    When a file named "build.ninja" with:
    """
    """
    Then I successfully run `turtle`

  Scenario: Run a rule without any input
    When a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello

    """
    Then I successfully run `turtle`
    And the stderr should contain exactly "hello"

  Scenario: Run a rule with an input
    When a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello bar

    """
    And a file named "bar" with ""
    Then I successfully run `turtle`
    And the stderr should contain exactly "hello"

  Scenario: Failt to run a rule with a missing input
    When a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello bar

    """
    Then I run `turtle`
    And the exit status should not be 0
