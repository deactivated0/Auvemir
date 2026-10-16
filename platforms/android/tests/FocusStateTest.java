package org.auvemir.app;

public final class FocusStateTest {
    private static void check(boolean condition,String message){if(!condition)throw new AssertionError(message);}
    public static void main(String[] args){
        FocusState focus=new FocusState();
        check(!focus.gain()&&!focus.granted&&!focus.requested,"An unrequested gain callback cannot acquire focus");
        focus.requestResult(true);
        focus.loss(true,true);
        check(!focus.granted&&!focus.shouldRelease(),"Transient pause must keep a pending focus request");
        focus.loss(true,false);
        check(focus.gain(),"Repeated transient callbacks must still resume the previously playing track");
        focus.loss(true,true);focus.userTransport();
        check(focus.shouldRelease(),"Explicit pause must release a pending request");
        check(!focus.gain(),"Explicit pause must cancel automatic resumption");
        focus.loss(true,true);focus.loss(false,false);
        check(!focus.gain(),"Permanent loss must cancel automatic resumption");
        focus.requestResult(false);
        check(!focus.granted&&!focus.requested,"Denied requests cannot count as granted");
        focus.requestResult(true);focus.released();
        check(!focus.granted&&!focus.requested,"Abandoned requests must clear both states");
        check(!focus.gain()&&!focus.granted&&!focus.requested,"A stale gain after abandonment cannot reacquire focus");
        System.out.println("Android audio focus event sequences passed");
    }
}
