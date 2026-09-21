Feature: Build statement

  Scenario: Rebuild an output on content update of an input
    Given a file named "build.ninja" with:
      """
      rule cp
        command = sh -c 'echo hello && cp $in $out'

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

  @turtle
  Scenario: Do not rebuild an output on timestamp update of an input
    Given a file named "build.ninja" with:
      """
      rule cp
        command = sh -c 'echo hello && cp $in $out'

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
        command = sh -c 'echo hello && cp bar $out'

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

  @turtle
  Scenario: Do not rebuild an output on timestamp update of an implicit input
    Given a file named "build.ninja" with:
      """
      rule cp
        command = sh -c 'echo hello && cp bar $out'

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
        command = sh -c '[ ! -r $out ] && cp bar $out'

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
        command = sh -c 'cp $in $out && cp $in baz'

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
        command = sh -c 'echo hello && cp $in $out'

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

  Scenario: Rebuild a dependent of an implicit output updated in the same run
    Given a file named "build.ninja" with:
      """
      rule gen
        command = sh -c 'cat $in > $out && cat $in > bar'

      rule cp
        command = cp $in $out

      build foo | bar: gen baz
      build qux: cp bar

      """
    And a file named "baz" with "1"
    When I successfully run `turtle`
    And a file named "baz" with "2"
    And I successfully run `turtle`
    Then the file named "qux" should contain "2"

  Scenario: Build an output from multiple outputs of a build
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      rule cat
        command = sh -c 'cat $in > $out'

      build foo bar: touch
      build baz: cat foo bar

      """
    When I successfully run `turtle baz`
    Then the file named "baz" should exist

  Scenario: Build a requested output without checking inputs of other builds
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out

      build foo: cp bar
      build baz: cp qux

      """
    And a file named "bar" with ""
    When I successfully run `turtle foo`
    Then the file named "foo" should exist

  Scenario: Do not rebuild an up-to-date output
    Given a file named "build.ninja" with:
      """
      rule cp
        command = sh -c '[ ! -r $out ] && cp $in $out'

      build foo: cp bar

      """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then I successfully run `turtle`

  Scenario: Do not rebuild an output with a variable in its implicit output
    Given a file named "build.ninja" with:
      """
      directory = foo

      rule touch
        command = sh -c '[ ! -e $out ] && touch $out $directory/baz'

      build bar | $directory/baz: touch

      """
    When I successfully run `turtle`
    Then I successfully run `turtle`

  Scenario: Rerun a failed rule
    Given a file named "build.ninja" with:
      """
      rule fail
        command = false

      build foo: fail

      """
    When I run `turtle`
    And the exit status should not be 0
    Then I run `turtle`
    And the exit status should not be 0

  Scenario: Escape newlines in a build statement
    Given a file named "build.ninja" with:
      """
      rule echo
        command = echo $in

      build foo: echo $
        bar $
        baz

      """
    And a file named "bar" with ""
    And a file named "baz" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "bar baz"

  Scenario: Escape a space in a build statement
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch "foo bar"

      build foo$ bar: touch

      """
    When I successfully run `turtle`
    Then the file named "foo bar" should exist

  @unix
  Scenario: Escape a colon in a build statement
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo$:bar: touch

      """
    When I successfully run `turtle`
    Then the file named "foo:bar" should exist
