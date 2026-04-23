Feature: C and C++ dependency discovery

  Scenario: Rebuild from a depfile-discovered header
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && printf '$out: $in header.h\n' > $out.d && cp $in $out
        depfile = $out.d
        deps = gcc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "header.h" with "#define FOO 1"
    When I successfully run `turtle`
    And a file named "header.h" with "#define FOO 2"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      building
      building
      """

  Scenario: Rebuild from an MSVC-discovered header
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'Note: including file: header.h\ncompiled\n' && cp $in $out
        deps = msvc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "header.h" with "#define FOO 1"
    When I successfully run `turtle`
    And a file named "header.h" with "#define FOO 2"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      compiled
      compiled
      """
