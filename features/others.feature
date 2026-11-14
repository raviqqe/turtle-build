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

  Scenario: Use a build file location with a parent directory component
    Given a directory named "foo"
    And a file named "build.ninja" with:
      """
      rule echo
        command = echo hello

      build foo: echo

      """
    When I successfully run `turtle -f foo/../build.ninja`
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

  Scenario: Prepare an output directory
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      build foo/bar: touch

      """
    When I successfully run `turtle`
    Then the file named "foo/bar" should exist

  Scenario: Skip comments
    Given a file named "build.ninja" with:
      """
      # foo

      """
    When I successfully run `turtle`
    Then the exit status should be 0

  Scenario: Specify an output
    Given a file named "build.ninja" with:
      """
      rule echo
        command = echo hello

      build foo: echo
      build bar: echo

      default foo bar

      """
    When I successfully run `turtle foo`
    Then the stdout should contain exactly "hello"

  @turtle @unix
  Scenario: Increase an open file limit
    Given a file named "build.ninja" with:
      """
      rule ulimit
        command = ulimit -Sn > $out

      build foo: ulimit

      """
    When I successfully run `sh -c 'ulimit -Sn 256 && turtle'`
    Then the file named "foo" should not contain exactly:
      """
      256
      """

  Scenario: Wait for a running command on failure of another command
    Given a file named "build.ninja" with:
      """
      rule fail
        command = sh -c 'sleep 0.2 && exit 1'

      rule slow
        command = sh -c 'sleep 1 && touch $out'

      build foo: fail
      build bar: slow

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the file named "bar" should exist

  Scenario: Run no command after failure of another command
    Given a file named "build.ninja" with:
      """
      rule fail
        command = sh -c 'sleep 0.2 && exit 1'

      rule slow
        command = sh -c 'sleep 1 && touch $out'

      rule touch
        command = touch $out

      build foo: fail
      build bar: slow
      build baz: touch bar

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the file named "bar" should exist
    And the file named "baz" should not exist

  Scenario: Do not rerun a command finished after failure of another command
    Given a file named "build.ninja" with:
      """
      rule check
        command = sh -c 'sleep 0.2 && test -e qux && touch $out'

      rule slow
        command = sh -c 'sleep 1 && printf hello >> log && touch $out'

      build foo: check
      build bar: slow

      """
    When I run `turtle`
    And a file named "qux" with ""
    And I successfully run `turtle`
    Then the file named "log" should contain exactly:
      """
      hello
      """
