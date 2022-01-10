Feature: Others 
  Scenario: Build nothing
    Given a file named "build.ninja" with:
    """
    """
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Do not build twice
    Given a file named "build.ninja" with:
    """
    rule cp
      command = echo hello && cp $in $out

    build foo: cp bar

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `turtle`
    Then the stderr should contain exactly "hello"
