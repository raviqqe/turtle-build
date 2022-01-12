Feature: Dynamic dependency
  Scenario: Use a dyndep file
    Given a file named "build.ninja" with:
    """
    rule cp
      command = cp $in $out
    rule dd
      command = echo hello && echo ninja_dyndep_version = 1 >> $out && echo build foo: dyndep >> $out

    build foo: cp bar || foo.dd
      dyndep = foo.dd
    build foo.dd: dd

    """
    And a file named "bar" with ""
    When I successfully run `turtle`
    Then the stderr should contain exactly "hello"
