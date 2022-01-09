Feature: Build
  Scenario: Use a subninja statement
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    subninja foo.ninja

    """
    Given a file named "foo.ninja" with:
    """
    build foo: hello

    """
    When I successfully run `turtle`
    Then the stderr should contain exactly "hello"
