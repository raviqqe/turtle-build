Feature: Default statement
  Scenario: Build a default output
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    build foo: hello
    build bar: hello

    default foo

    """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Build a default output in a child build file
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    subninja foo.ninja

    """
    And a file named "foo.ninja" with:
    """
    build foo: hello
    build bar: hello

    default foo

    """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"
