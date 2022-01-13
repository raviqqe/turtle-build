Feature: Rule statement
  Scenario: Run a rule without any input
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello

    """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

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

  Scenario: Run a rule with an input variable
    Given a file named "build.ninja" with:
    """
    rule echo
      command = echo $in

    build foo: echo bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "bar"

  Scenario: Run a rule with an output variable
    Given a file named "build.ninja" with:
    """
    rule echo
      command = echo $out

    build foo: echo bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "foo"

  Scenario: Run a rule with a custom variable
    Given a file named "build.ninja" with:
    """
    rule echo
      command = echo $x

    build foo: echo
      x = hello

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Run a rule with two custom variables
    Given a file named "build.ninja" with:
    """
    rule echo
      command = echo $x $y

    build foo: echo
      x = hello
      y = world

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello world"

  Scenario: Run a phony rule
    Given a file named "build.ninja" with:
    """
    rule touch
      command = touch $out

    build foo: touch
    build bar: phony foo

    default bar

    """
    When I successfully run `turtle`
    Then the file named "foo" should exist
