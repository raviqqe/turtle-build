Feature: Build
  Scenario: Build with a build file
    Given a file named "build.ninja" with:
    """
    """
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Run a rule without any input
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello

    """
    When I successfully run `turtle`
    Then the stderr should contain exactly "hello"

  Scenario: Run a rule with an input
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Fail to run a rule with a missing input
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello bar

    """
    When I run `turtle`
    Then the exit status should not be 0

  Scenario: Run a rule with variables
    Given a file named "build.ninja" with:
    """
    rule echo
      command = echo $out $in

    build foo: echo bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stderr should contain exactly "foo bar"
