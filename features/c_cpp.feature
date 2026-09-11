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
    And the file named "foo.o.d" should not exist

  Scenario: Rebuild from a header discovered by a depfile without deps
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && printf '$out: $in header.h\n' > $out.d && cp $in $out
        depfile = $out.d

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
    And the file named "foo.o.d" should exist

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

  Scenario: Discover a header via a custom msvc_deps_prefix declared on the rule
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'Remarque : inclusion du fichier : header.h\ncompiled\n' && cp $in $out
        deps = msvc
        msvc_deps_prefix = Remarque : inclusion du fichier : 

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

  Scenario: Discover a header via a custom msvc_deps_prefix declared at the top level
    Given a file named "build.ninja" with:
      """
      msvc_deps_prefix = Remarque : inclusion du fichier : 

      rule cc
        command = printf 'Remarque : inclusion du fichier : header.h\ncompiled\n' && cp $in $out
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

  Scenario: Prefer a build-level msvc_deps_prefix over a conflicting rule-level one
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'Remarque : inclusion du fichier : header.h\ncompiled\n' && cp $in $out
        deps = msvc
        msvc_deps_prefix = WRONG: 

      build foo.o: cc source.c
        msvc_deps_prefix = Remarque : inclusion du fichier : 

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

  Scenario: Do not rebuild a gcc-discovered header when nothing changes
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && printf '$out: $in header.h\n' > $out.d && cat $in > $out
        depfile = $out.d
        deps = gcc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "header.h" with "#define FOO 1"
    When I successfully run `turtle`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      building
      """

  Scenario: Do not rebuild an MSVC-discovered header when nothing changes
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'Note: including file: header.h\ncompiled\n' && cat $in > $out
        deps = msvc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "header.h" with "#define FOO 1"
    When I successfully run `turtle`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      compiled
      """

  Scenario: Tolerate a command that does not write its declared depfile
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && cp $in $out
        depfile = $out.d
        deps = gcc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    When I successfully run `turtle`
    Then the file named "foo.o" should exist

  Scenario: Recover after a discovered header is deleted
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
    And I remove the file "header.h"
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      building
      building
      """

  @turtle
  Scenario: Stabilize after a discovered header is deleted
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
    And I remove the file "header.h"
    And I successfully run `turtle`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      building
      building
      """

  Scenario: Build foo.o on a clean checkout before its generated header exists
    Given a file named "build.ninja" with:
      """
      rule gen
        command = printf '#define G 1\n' > $out

      rule cc
        command = printf 'building\n' && printf '$out: $in gen.h\n' > $out.d && cp $in $out
        depfile = $out.d
        deps = gcc

      build gen.h: gen
      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    When I successfully run `turtle foo.o`
    Then the file named "foo.o" should exist

  # TODO Remove this scenario once newly discovered dependencies are recorded
  # without being built after the command that discovered them.
  @turtle
  Scenario: Build a depfile-discovered generated header on a clean checkout
    Given a file named "build.ninja" with:
      """
      rule gen
        command = printf '#define G 1\n' > $out

      rule cc
        command = printf 'building\n' && printf '$out: $in gen.h\n' > $out.d && cp $in $out
        depfile = $out.d
        deps = gcc

      build gen.h: gen
      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    When I successfully run `turtle foo.o`
    Then the file named "foo.o" should exist
    And the file named "gen.h" should exist

  @turtle
  Scenario: Report an error for a cycle through a discovered dependency instead of hanging
    Given a file named "build.ninja" with:
      """
      rule gen
        command = cp $in $out

      rule cc
        command = printf '$out: $in gen.h\n' > $out.d && cp $in $out
        depfile = $out.d
        deps = gcc

      build gen.h: gen foo.o
      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    When I run `turtle foo.o`
    Then the exit status should not be 0
    When I run `turtle foo.o`
    Then the exit status should not be 0

  Scenario: Accept a header path with an escaped space in a depfile
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && printf '$out: $in my\\ header.h\n' > $out.d && cat $in > $out
        depfile = $out.d
        deps = gcc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "my header.h" with "#define FOO 1"
    When I successfully run `turtle`
    And I successfully run `turtle`
    Then the stdout should contain exactly:
      """
      building
      """

  Scenario: Ignore a depfile set alongside deps = msvc
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'Note: including file: header.h\ncompiled\n' && printf '$out: $in nonexistent.h\n' > $out.d && cp $in $out
        depfile = $out.d
        deps = msvc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    And a file named "header.h" with "#define FOO 1"
    When I successfully run `turtle`
    Then the stdout should contain exactly "compiled"

  Scenario: Reject deps = gcc without a depfile
    Given a file named "build.ninja" with:
      """
      rule cc
        command = printf 'building\n' && cp $in $out
        deps = gcc

      build foo.o: cc source.c

      """
    And a file named "source.c" with "int main(void) { return 0; }"
    When I run `turtle`
    Then the exit status should not be 0
