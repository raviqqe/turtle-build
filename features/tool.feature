Feature: Tool
  Scenario: Clean dead outputs
    Given a file named "build.ninja" with:
    """
    rule touch
      command = touch $out

    build foo: touch

    """
    And I successfully run `turtle`
    When a file named "build.ninja" with:
    """

    """
    And I successfully run `turtle -t cleandead`
    Then the file "foo" should not exist

  Scenario: Do not clean live outputs
    Given a file named "build.ninja" with:
    """
    rule touch
      command = touch $out

    build foo: touch

    """
    And I successfully run `turtle`
    When I successfully run `turtle -t cleandead`
    Then the file "foo" should exist
