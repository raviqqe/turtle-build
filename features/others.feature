Feature: Others
  Scenario: Build nothing
    Given a file named "build.ninja" with:
    """
    """
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Do not rebuild an up-to-date output
    Given a file named "build.ninja" with:
    """
    rule cp
      command = [ ! -r $out ] && cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then I successfully run `turtle`

  Scenario: Rebuild a stale output
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `touch bar`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    hello
    """

  Scenario: Chain rebuilds
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp $in $out

    build bar: cp baz
    build foo: cp bar

    """
    And a file named "baz" with ""
    When I successfully run `turtle`
    And I successfully run `touch baz`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    hello
    hello
    hello
    """

  Scenario: Use a custom build file location
    Given a file named "foo.ninja" with:
    """
    rule echo
      command = echo hello

    build foo: echo

    """
    When I successfully run `turtle -f foo.ninja`
    Then the stdout should contain exactly "hello"

  Scenario: Rerun a failed rule
    Given a file named "build.ninja" with:
    """
    rule fail
      command = exit 1

    build foo: fail

    """
    When I run `turtle`
    And the exit status should not be 0
    Then I run `turtle`
    And the exit status should not be 0

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
