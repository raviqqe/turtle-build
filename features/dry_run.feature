Feature: Dry run

  Scenario: Run no command
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo: touch

      """
    When I successfully run `turtle -n`
    Then the file named "foo" should not exist

  Scenario: Build an output after a dry run
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo: touch

      """
    When I successfully run `turtle -n`
    And I successfully run `turtle`
    Then the file named "foo" should exist

  Scenario: Run no command of a dependent of a stale build
    Given a file named "build.ninja" with:
      """
      rule cp
        command = sh -c 'echo hello && cp $in $out'

      build bar: cp baz
      build foo: cp bar

      """
    And a file named "baz" with ""
    When I successfully run `turtle`
    And a file named "baz" with "baz"
    And I successfully run `turtle -n`
    Then the stdout should contain exactly:
      """
      hello
      hello
      """

  Scenario: Fail with a missing input
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out

      build foo: cp bar

      """
    When I run `turtle -n`
    Then the exit status should not be 0

  @turtle
  Scenario: Show a description
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        description = touching $out

      build foo: touch

      """
    When I successfully run `turtle -n`
    Then the stderr should contain exactly "touching foo"

  @turtle
  Scenario: Show a command with a debug option
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo: touch

      """
    When I successfully run `turtle -n --debug`
    Then the stderr should contain exactly "turtle: command: touch foo"

  @turtle
  Scenario: Show descriptions of builds never run
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
        description = copying $out

      build bar: cp baz
      build foo: cp bar

      """
    And a file named "baz" with ""
    When I successfully run `turtle -n`
    Then the stderr should contain exactly:
      """
      copying bar
      copying foo
      """

  @turtle
  Scenario: Show no description of an up-to-date build
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
        description = copying $out

      build foo: cp bar

      """
    And a file named "bar" with ""
    When I successfully run `turtle`
    And I successfully run `turtle -n`
    Then the stderr from "turtle -n" should contain exactly ""

  @turtle
  Scenario: Show a description of a dependent of a stale build
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
        description = copying $out

      build bar: cp baz
      build foo: cp bar

      """
    And a file named "baz" with ""
    When I successfully run `turtle`
    And a file named "baz" with "baz"
    And I successfully run `turtle -n`
    Then the stderr from "turtle -n" should contain exactly:
      """
      copying bar
      copying foo
      """

  @turtle
  Scenario: Show no description of a dependent of a stale order-only input
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
        description = copying $out
      rule touch
        command = touch $out
        description = touching $out

      build bar: cp baz
      build foo: touch || bar

      """
    And a file named "baz" with ""
    When I successfully run `turtle`
    And a file named "baz" with "baz"
    And I successfully run `turtle -n`
    Then the stderr from "turtle -n" should contain exactly "copying bar"

  @turtle
  Scenario: Show a description of a dependent of a stale phony input
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        description = touching $out

      build bar: phony
      build foo: touch bar

      """
    When I successfully run `turtle`
    And I successfully run `turtle -n`
    Then the stderr from "turtle -n" should contain exactly "touching foo"

  @turtle
  Scenario: Show no description of a dependent of an up-to-date phony input
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        description = touching $out

      build bar: phony baz
      build foo: touch bar

      """
    And a file named "baz" with ""
    When I successfully run `turtle`
    And I successfully run `turtle -n`
    Then the stderr from "turtle -n" should contain exactly ""

  @turtle
  Scenario: Show a description of a build with a dyndep file never built
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        description = touching $out
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep >> $out'
        description = generating $out

      build foo: touch || foo.dd
        dyndep = foo.dd
      build foo.dd: dd

      """
    When I successfully run `turtle -n`
    Then the stderr should contain exactly:
      """
      generating foo.dd
      touching foo
      """

  @turtle
  Scenario: Create no output directory
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo/bar: touch

      """
    When I successfully run `turtle -n`
    Then the directory named "foo" should not exist

  @turtle
  Scenario: Fail to use a tool
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo: touch

      """
    When I run `turtle -n -t cleandead`
    Then the exit status should not be 0
