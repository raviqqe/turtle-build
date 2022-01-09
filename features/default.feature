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
    Then the stderr should contain exactly "hello"

