Feature: Subninja statement

  Scenario: Build an output in a child build file
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
    Then the stdout should contain exactly "hello"

  Scenario: Build an output in a grandchild build file
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
    Then the stdout should contain exactly "hello"

  Scenario: Resolve a path relative to a working directory
    Given a file named "build.ninja" with:
      """
      rule hello
        command = echo hello

      subninja foo/foo.ninja

      """
    And a file named "foo/foo.ninja" with:
      """
      subninja bar.ninja

      """
    And a file named "bar.ninja" with:
      """
      build foo: hello

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"
