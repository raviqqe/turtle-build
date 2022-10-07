Feature: Command line options
  @turtle
  Scenario: Set a log prefix
    Given a file named "build.ninja" with:
    """
    rule touch
      command = touch $out

    build foo: touch

    """
    When I successfully run `turtle --debug`
    Then the stderr should contain "touch"

  @turtle
  Scenario: Set a log prefix
    Given a file named "build.ninja" with:
    """
    rule
    """
    When I run `turtle --log-prefix tomato`
    Then the exit status should not be 0
    And the stderr should contain "tomato"
