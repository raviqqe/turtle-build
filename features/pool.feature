Feature: Pool statement

  Scenario: Run builds in a pool
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1

      rule lock
        command = sh -c 'mkdir lock && sleep 0.1 && rmdir lock && touch $out'
        pool = foo

      build bar: lock
      build baz: lock
      build qux: lock

      """
    When I successfully run `turtle -j 3`
    Then the file named "bar" should exist
    And the file named "baz" should exist
    And the file named "qux" should exist

  Scenario: Run builds in a pool set in build statements
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1

      rule lock
        command = sh -c 'mkdir lock && sleep 0.1 && rmdir lock && touch $out'

      build bar: lock
        pool = foo
      build baz: lock
        pool = foo

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Run builds in a pool of depth two concurrently
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 2

      rule wait
        command = sh -c 'touch $out.started && for _ in $$(seq 100); do if [ -e $other.started ]; then touch $out; exit; fi; sleep 0.05; done; exit 1'
        pool = foo

      build bar: wait
        other = baz
      build baz: wait
        other = bar

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Run builds in a pool of zero depth concurrently
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 0

      rule wait
        command = sh -c 'touch $out.started && for _ in $$(seq 100); do if [ -e $other.started ]; then touch $out; exit; fi; sleep 0.05; done; exit 1'
        pool = foo

      build bar: wait
        other = baz
      build baz: wait
        other = bar

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Exempt builds from a pool of their rule
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1

      rule wait
        command = sh -c 'touch $out.started && for _ in $$(seq 100); do if [ -e $other.started ]; then touch $out; exit; fi; sleep 0.05; done; exit 1'
        pool = foo

      build bar: wait
        other = baz
        pool =
      build baz: wait
        other = bar
        pool =

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Share a pool with a child build file
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1

      rule lock
        command = sh -c 'mkdir lock && sleep 0.1 && rmdir lock && touch $out'
        pool = foo

      build bar: lock

      subninja baz.ninja

      """
    And a file named "baz.ninja" with:
      """
      build baz: lock

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Use a pool declared in a child build file
    Given a file named "build.ninja" with:
      """
      subninja foo.ninja

      rule touch
        command = touch $out
        pool = foo

      build bar: touch

      """
    And a file named "foo.ninja" with:
      """
      pool foo
        depth = 1

      """
    When I successfully run `turtle`
    Then the file named "bar" should exist

  Scenario: Build a phony output in a console pool
    Given a file named "build.ninja" with:
      """
      build foo: phony
        pool = console

      """
    When I successfully run `turtle foo`
    Then the exit status should be 0

  Scenario: Run builds in a console pool
    Given a file named "build.ninja" with:
      """
      rule lock
        command = sh -c 'mkdir lock && sleep 0.1 && rmdir lock && touch $out'
        pool = console

      build bar: lock
      build baz: lock

      """
    When I successfully run `turtle -j 2`
    Then the file named "bar" should exist
    And the file named "baz" should exist

  Scenario: Read standard input in a console pool
    Given a file named "build.ninja" with:
      """
      rule cat
        command = sh -c 'cat > $out'
        pool = console

      build foo: cat

      """
    When I successfully run `sh -c 'echo hello | turtle'`
    Then the file named "foo" should contain "hello"

  Scenario: Write standard error in a console pool
    Given a file named "build.ninja" with:
      """
      rule echo
        command = sh -c 'echo hello >&2'
        pool = console

      build foo: echo

      """
    When I successfully run `turtle`
    Then the stderr should contain "hello"

  Scenario: Do not read standard input outside a console pool
    Given a file named "build.ninja" with:
      """
      rule cat
        command = sh -c 'cat > $out'

      build foo: cat

      """
    When I successfully run `sh -c 'echo hello | turtle'`
    Then the file named "foo" should not contain "hello"

  Scenario: Write outputs of other builds after a build in a console pool
    Given a file named "build.ninja" with:
      """
      rule console
        command = sh -c 'sleep 1 && echo console'
        pool = console

      rule echo
        command = sh -c 'echo normal && touch $out'

      rule sleep
        command = sh -c 'sleep 0.5 && touch $out'

      build foo: console
      build bar: sleep
      build baz: echo bar

      """
    When I successfully run `turtle -j 4 foo baz`
    Then the stdout should contain exactly:
      """
      console
      normal
      """

  @turtle
  Scenario: Write a description before an output of a build in a console pool
    Given a file named "build.ninja" with:
      """
      rule echo
        command = echo hello
        description = foo
        pool = console

      build foo: echo

      """
    When I successfully run `sh -c 'turtle 2>&1'`
    Then the stdout should contain exactly:
      """
      foo
      hello
      """

  Scenario: Fail a build in a console pool
    Given a file named "build.ninja" with:
      """
      rule fail
        command = false
        pool = console

      build foo: fail

      """
    When I run `turtle`
    Then the exit status should not be 0

  Scenario: Fail to use an unknown pool
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        pool = foo

      build bar: touch

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the output should contain "foo"

  Scenario: Fail to use a pool declared after a child build file
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out

      subninja bar.ninja

      pool foo
        depth = 1

      """
    And a file named "bar.ninja" with:
      """
      build baz: touch
        pool = foo

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the output should contain "foo"

  Scenario: Fail to declare a pool twice
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1

      pool foo
        depth = 2

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the output should contain "foo"

  Scenario: Fail to declare a console pool
    Given a file named "build.ninja" with:
      """
      pool console
        depth = 1

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the output should contain "console"

  Scenario: Fail to declare a pool without a depth
    Given a file named "build.ninja" with:
      """
      pool foo

      """
    When I run `turtle`
    Then the exit status should not be 0

  Scenario: Fail to declare a pool with a negative depth
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = -1

      """
    When I run `turtle`
    Then the exit status should not be 0

  Scenario: Fail to declare a pool with an unknown variable
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = 1
        bar = 2

      """
    When I run `turtle`
    Then the exit status should not be 0

  @turtle
  Scenario: Fail to declare a pool with an invalid depth
    Given a file named "build.ninja" with:
      """
      pool foo
        depth = bar

      """
    When I run `turtle`
    Then the exit status should not be 0
    And the stderr should contain "has invalid depth"
