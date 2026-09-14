Feature: Variable definition

  Scenario: Expand a variable on definition
    Given a file named "build.ninja" with:
      """
      x = hello
      y = $x
      x = world

      rule echo
        command = echo $y

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Expand a nested variable
    Given a file named "build.ninja" with:
      """
      x = hello
      y = $x world

      rule echo
        command = echo $y

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello world"

  Scenario: Append a value to a variable
    Given a file named "build.ninja" with:
      """
      x = hello
      x = $x world

      rule echo
        command = echo $x

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello world"

  Scenario: Escape dollar signs in a variable
    Given a file named "build.ninja" with:
      """
      x = $$y $$$$

      rule echo
        command = echo '$x'

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "$y $$"

  Scenario: Escape a newline in a variable
    Given a file named "build.ninja" with:
      """
      x = hello $
          world

      rule echo
        command = echo $x

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello world"

  Scenario: Define a variable prefixed with a keyword
    Given a file named "build.ninja" with:
      """
      default_message = hello

      rule echo
        command = echo $default_message

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stdout should contain exactly "hello"

  Scenario: Expand variables in build paths
    Given a file named "build.ninja" with:
      """
      directory = foo

      rule touch
        command = touch $out

      build $directory/bar ${directory}/baz: touch

      default $directory/bar

      """
    When I successfully run `turtle`
    Then the file named "foo/bar" should exist
    And the file named "foo/baz" should exist
