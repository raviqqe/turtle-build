Feature: Dynamic dependency

  Scenario: Use a dyndep file
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
      rule dd
        command = sh -c 'echo ok && echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep >> $out'

      build foo: cp bar || foo.dd
        dyndep = foo.dd
      build foo.dd: dd

      """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "ok"

  Scenario: Use a dyndep file with an addtional input
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
      rule cp
        command = sh -c 'echo ok && cp $in $out'
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep "|" bar >> $out'

      build foo: touch || foo.dd
        dyndep = foo.dd
      build foo.dd: dd
      build bar: cp baz

      """
    And a file named "baz" with ""
    When I successfully run `turtle`
    Then the stdout should contain exactly "ok"

  Scenario: Rebuild a dependent of a dynamic input updated in the same run
    Given a file named "build.ninja" with:
      """
      rule cp
        command = cp $in $out
      rule cat
        command = sh -c 'cat bar > $out'
      rule dd
        command = sh -c 'printf "ninja_dyndep_version = 1\nbuild foo: dyndep | bar\n" > $out'

      build foo: cat || foo.dd
        dyndep = foo.dd
      build foo.dd: dd
      build bar: cp baz

      """
    And a file named "baz" with "1"
    When I successfully run `turtle`
    And a file named "baz" with "2"
    And I successfully run `turtle`
    Then the file named "foo" should contain "2"

  Scenario: Use a dyndep file declared in a rule
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
        dyndep = $out.dd
      rule cp
        command = sh -c 'echo ok && cp $in $out'
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep "|" bar >> $out'

      build foo: touch || foo.dd
      build foo.dd: dd
      build bar: cp baz

      """
    And a file named "baz" with ""
    When I successfully run `turtle foo`
    Then the stdout should contain exactly "ok"

  Scenario: Use a dyndep file naming an implicit output
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
      rule cp
        command = sh -c 'echo ok && cp $in $out'
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build baz: dyndep "|" bar >> $out'

      build foo | baz: touch || foo.dd
        dyndep = foo.dd
      build foo.dd: dd
      build bar: cp qux

      """
    And a file named "qux" with ""
    When I successfully run `turtle foo`
    Then the stdout should contain exactly "ok"

  Scenario: Use a dyndep file shared by builds
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
      rule cp
        command = sh -c 'echo $out && cp $in $out'
      rule dd
        command = sh -c 'printf "ninja_dyndep_version = 1\nbuild foo: dyndep | baz\nbuild bar: dyndep | qux\n" > $out'

      build foo: touch || foo.dd
        dyndep = foo.dd
      build bar: touch foo || foo.dd
        dyndep = foo.dd
      build foo.dd: dd
      build baz: cp quux
      build qux: cp quux

      """
    And a file named "quux" with ""
    When I successfully run `turtle bar`
    Then the stdout should contain "baz"
    And the stdout should contain "qux"

  Scenario: Fail to use a dyndep file that is not an input
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep >> $out'

      build foo: touch
        dyndep = foo.dd
      build foo.dd: dd

      """
    When I run `turtle foo`
    Then the exit status should not be 0
    And the output should contain "foo.dd"

  Scenario: Fail to use a dyndep file naming an unknown output
    Given a file named "build.ninja" with:
      """
      rule touch
        command = touch $out
      rule dd
        command = sh -c 'echo ninja_dyndep_version = 1 >> $out && echo build baz: dyndep "|" bar >> $out'

      build foo: touch || foo.dd
        dyndep = foo.dd
      build foo.dd: dd

      """
    When I run `turtle foo`
    Then the exit status should not be 0
    And the output should contain "baz"
