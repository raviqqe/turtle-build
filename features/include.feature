Feature: Include statement

  Scenario: Use a variable in a child build file
    Given a file named "build.ninja" with:
      """
      include foo.ninja

      rule echo
        command = echo $x

      build foo: echo

      """
    And a file named "foo.ninja" with:
      """
      x = hello

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Use a rule in a child build file
    Given a file named "build.ninja" with:
      """
      include foo.ninja

      build foo: hello

      """
    And a file named "foo.ninja" with:
      """
      rule hello
        command = echo hello

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Resolve a path relative to a working directory
    Given a file named "build.ninja" with:
      """
      include foo/foo.ninja

      rule echo
        command = echo $x

      build foo: echo

      """
    And a file named "foo/foo.ninja" with:
      """
      include bar.ninja

      """
    And a file named "bar.ninja" with:
      """
      x = hello

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"
