Feature: Others
  Scenario: Build nothing
    Given a file named "build.ninja" with:
    """
    """
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Use a custom build file location
    Given a file named "foo.ninja" with:
    """
    rule echo
      command = echo hello

    build foo: echo

    """
    When I successfully run `turtle -f foo.ninja`
    Then the stdout should contain exactly "hello"

  Scenario: Change a directory first
    Given a directory named "foo"
    And I cd to "foo"
    And a file named "build.ninja" with:
    """
    rule cp
      command = cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    And I cd to ".."
    When I successfully run `turtle -C foo`
    Then a file named "foo/foo" should exist
