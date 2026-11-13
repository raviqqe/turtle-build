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

  @turtle
  Scenario: Kill a command on failure of another command
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
    Then the exit status should be 1
    And I successfully run `sleep 1.5`
    And the file named "bar" should not exist

  @turtle @unix
  Scenario: Kill a descendant of a command on failure of another command
    Given a file named "build.ninja" with:
      """
      rule fail
        command = sleep 0.2 && exit 1

      rule slow
        command = (sleep 1 && touch $out) > /dev/null 2>&1 & wait

      build foo: fail
      build bar: slow

      """
    When I run `turtle`
    Then the exit status should be 1
    And I successfully run `sleep 1.5`
    And the file named "bar" should not exist

  @turtle @unix
  Scenario: Kill a command on a termination signal
    Given a file named "build.ninja" with:
      """
      rule slow
        command = (sleep 1 && touch $out) > /dev/null 2>&1 & wait

      build foo: slow

      """
    When I run the following script:
      """
      turtle &
      sleep 0.5
      kill $!
      wait $!
      """
    Then the exit status should be 143
    And I successfully run `sleep 1`
    And the file named "foo" should not exist
