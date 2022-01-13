Feature: Build statement
  Scenario: Rebuild an output on update of an input
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
    Then the stderr should contain exactly:
    """
    hello
    hello
    """

  Scenario: Rebuild an output on update of an implicit input
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
    Then the stderr should contain exactly:
    """
    hello
    hello
    """

  Scenario: Do not rebuild an output on update of an order-only input
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp bar $out

    build foo: cp || bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `touch bar`
    And I successfully run `turtle`
    Then the stderr should contain exactly "hello"

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
