Feature: Build statement
  Scenario: Rebuild an output on content update of an input
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And a file named "bar" with "bar"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    hello
    """

  Scenario: Do not rebuild an output on timestamp update of an input
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
    """

  Scenario: Rebuild an output on content update of an implicit input
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp bar $out

    build foo: cp | bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And a file named "bar" with "bar"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    hello
    """

  Scenario: Do not rebuild an output on timestamp update of an implicit input
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp bar $out

    build foo: cp | bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `touch bar`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    """

  Scenario: Do not rebuild an output on update of an order-only input
    Given a file named "build.ninja" with:
    """
    rule cp
      command = [ ! -r $out ] && cp bar $out

    build foo: cp || bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `touch bar`
    Then I successfully run `turtle`

  Scenario: Rebuild a deleted output
    Given a file named "build.ninja" with:
    """
    rule cp
      command = cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `rm foo`
    And I successfully run `turtle`
    Then the file named "foo" should exist

  Scenario: Rebuild a deleted implicit output
    Given a file named "build.ninja" with:
    """
    rule cp
      command = cp $in $out && cp $in baz

    build foo | baz: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `rm baz`
    And I successfully run `turtle`
    Then the file named "baz" should exist

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
    And a file named "baz" with "baz"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
    """
    hello
    hello
    hello
    hello
    """

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
