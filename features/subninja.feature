Feature: Build
  Scenario: Use a subninja statement
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    subninja foo.ninja

    """
    And a file named "foo.ninja" with:
    """
    build foo: hello

    """
    When I successfully run `turtle`
    Then the stderr should contain exactly "hello"

  Scenario: Use a nested subninja statement
    Given a file named "build.ninja" with:
    """
    rule hello
      command = echo hello

    subninja foo.ninja

    """
    And a file named "foo.ninja" with:
    """
    subninja bar.ninja

    """
    And a file named "bar.ninja" with:
    """
    build foo: hello

    """
    When I successfully run `turtle`
    Then the stderr should contain exactly "hello"
